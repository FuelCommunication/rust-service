use std::time::Duration;

use bytes::BytesMut;
use pingora::http::ResponseHeader;
use pingora::prelude::{HTTPStatus, Session};
use serde::{Deserialize, Serialize};
use tonic::transport::Channel;

use crate::{
    PingoraResult,
    config::Config,
    proto::{self, auth_service_client::AuthServiceClient},
};

const GRPC_TIMEOUT: Duration = Duration::from_secs(10);

pub struct AuthContext<'a> {
    pub origin: Option<&'a str>,
    pub allowed_origins: &'a [String],
    pub request_id: &'a str,
    pub max_body_size: usize,
    pub config: &'a Config,
}

fn urldecode(s: &str) -> Option<String> {
    let mut result = Vec::with_capacity(s.len());
    let mut bytes = s.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let hi = bytes.next().and_then(hex_val)?;
            let lo = bytes.next().and_then(hex_val)?;
            result.push(hi << 4 | lo);
        } else if b == b'+' {
            result.push(b' ');
        } else {
            result.push(b);
        }
    }
    String::from_utf8(result).ok()
}

fn urlencode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            _ => {
                result.push('%');
                result.push(char::from(HEX_CHARS[(b >> 4) as usize]));
                result.push(char::from(HEX_CHARS[(b & 0x0F) as usize]));
            }
        }
    }
    result
}

const HEX_CHARS: [u8; 16] = *b"0123456789ABCDEF";

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[derive(Deserialize)]
struct LoginBody {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct RegisterBody {
    email: String,
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct RefreshBody {
    refresh_token: String,
}

#[derive(Deserialize)]
struct LogoutBody {
    refresh_token: String,
}

#[derive(Serialize)]
struct UserDto {
    id: String,
    email: String,
    username: String,
    avatar_url: Option<String>,
    bio: Option<String>,
}

#[derive(Serialize)]
struct SessionDto {
    access_token: String,
    #[serde(rename = "tokenType")]
    token_type: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
    #[serde(rename = "expiresIn")]
    expires_in: i64,
}

#[derive(Serialize)]
struct AuthResponseDto {
    user: UserDto,
    session: SessionDto,
}

#[derive(Serialize)]
struct ErrorDto {
    error: String,
}

async fn read_full_body(session: &mut Session, max_size: usize) -> Result<bytes::Bytes, String> {
    let mut buf = BytesMut::new();
    loop {
        match session.read_request_body().await {
            Ok(Some(chunk)) => {
                buf.extend_from_slice(&chunk);
                if buf.len() > max_size {
                    return Err("Request body too large".into());
                }
            }
            Ok(None) => break,
            Err(e) => return Err(format!("Failed to read body: {e}")),
        }
    }
    Ok(buf.freeze())
}

async fn respond_json<T: Serialize>(
    session: &mut Session,
    status: u16,
    body: &T,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let json = serde_json::to_vec(body)
        .map_err(|e| pingora::Error::explain(HTTPStatus(500), format!("JSON serialization error: {e}")))?;

    let mut header = ResponseHeader::build(status, None)?;
    header.insert_header("Content-Type", "application/json")?;
    header.insert_header("Content-Length", json.len().to_string())?;
    header.insert_header("X-Request-Id", request_id)?;
    crate::insert_cors_headers(&mut header, origin, allowed_origins)?;
    session.write_response_header(Box::new(header), false).await?;
    session.write_response_body(Some(bytes::Bytes::from(json)), true).await?;
    Ok(true)
}

async fn respond_error(
    session: &mut Session,
    status: u16,
    message: &str,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    respond_json(
        session,
        status,
        &ErrorDto {
            error: message.to_string(),
        },
        origin,
        allowed_origins,
        request_id,
    )
    .await
}

async fn respond_redirect(session: &mut Session, location: &str, request_id: &str) -> PingoraResult<bool> {
    let mut header = ResponseHeader::build(302, None)?;
    header.insert_header("Location", location)?;
    header.insert_header("X-Request-Id", request_id)?;
    session.write_response_header(Box::new(header), true).await?;
    Ok(true)
}

fn grpc_status_to_http(code: tonic::Code) -> u16 {
    match code {
        tonic::Code::InvalidArgument => 400,
        tonic::Code::Unauthenticated => 401,
        tonic::Code::PermissionDenied => 403,
        tonic::Code::NotFound => 404,
        tonic::Code::AlreadyExists => 409,
        tonic::Code::ResourceExhausted => 429,
        _ => 500,
    }
}

fn tokens_to_session(tokens: &proto::AuthTokens) -> Result<SessionDto, &'static str> {
    let now = chrono::Utc::now().timestamp();
    let access_expires_at = tokens
        .access_expires_at
        .as_ref()
        .map(|t| t.seconds)
        .ok_or("Auth response missing token expiry")?;

    Ok(SessionDto {
        access_token: tokens.access_token.clone(),
        token_type: "Bearer".to_string(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: access_expires_at - now,
    })
}

async fn post_grpc_handler<B, R, F, Fut>(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
    handler: F,
) -> PingoraResult<bool>
where
    B: serde::de::DeserializeOwned,
    F: FnOnce(AuthServiceClient<Channel>, B) -> Fut,
    Fut: std::future::Future<Output = Result<R, tonic::Status>>,
    R: HandleResult,
{
    let body = match read_full_body(session, ctx.max_body_size).await {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &e, ctx.origin, ctx.allowed_origins, ctx.request_id).await,
    };

    let parsed: B = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => {
            return respond_error(
                session,
                400,
                &format!("Invalid JSON: {e}"),
                ctx.origin,
                ctx.allowed_origins,
                ctx.request_id,
            )
            .await;
        }
    };

    let client = auth_client.clone();
    match handler(client, parsed).await {
        Ok(result) => result.into_response(session, ctx).await,
        Err(status) => {
            let http_code = grpc_status_to_http(status.code());
            respond_error(
                session,
                http_code,
                status.message(),
                ctx.origin,
                ctx.allowed_origins,
                ctx.request_id,
            )
            .await
        }
    }
}

trait HandleResult {
    fn into_response(
        self,
        session: &mut Session,
        ctx: &AuthContext<'_>,
    ) -> impl std::future::Future<Output = PingoraResult<bool>> + Send;
}

pub async fn handle_auth_route(
    session: &mut Session,
    path: &str,
    method: &str,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
) -> PingoraResult<bool> {
    if method == "GET" {
        return match path {
            "/access/oauth/google" => {
                handle_oauth_start(
                    session,
                    auth_client,
                    proto::OAuthProvider::OauthProviderGoogle,
                    "google",
                    ctx.config,
                    ctx.request_id,
                )
                .await
            }
            "/access/oauth/github" => {
                handle_oauth_start(
                    session,
                    auth_client,
                    proto::OAuthProvider::OauthProviderGithub,
                    "github",
                    ctx.config,
                    ctx.request_id,
                )
                .await
            }
            "/access/oauth/callback" => handle_oauth_callback(session, auth_client, ctx.config, ctx.request_id).await,
            _ => {
                respond_error(
                    session,
                    405,
                    "Method not allowed",
                    ctx.origin,
                    ctx.allowed_origins,
                    ctx.request_id,
                )
                .await
            }
        };
    }

    if method != "POST" {
        return respond_error(
            session,
            405,
            "Method not allowed",
            ctx.origin,
            ctx.allowed_origins,
            ctx.request_id,
        )
        .await;
    }

    let content_type = session
        .req_header()
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.to_ascii_lowercase().starts_with("application/json") {
        return respond_error(
            session,
            415,
            "Content-Type must be application/json",
            ctx.origin,
            ctx.allowed_origins,
            ctx.request_id,
        )
        .await;
    }

    match path {
        "/access/login" => handle_login(session, auth_client, ctx).await,
        "/access/register" => handle_register(session, auth_client, ctx).await,
        "/access/refresh" => handle_refresh(session, auth_client, ctx).await,
        "/access/logout" => handle_logout(session, auth_client, ctx).await,
        _ => respond_error(session, 404, "Not found", ctx.origin, ctx.allowed_origins, ctx.request_id).await,
    }
}

struct AuthUserResult {
    user_id: String,
    email: String,
    username: String,
    tokens: Option<proto::AuthTokens>,
    status: u16,
}

struct TokensResult(proto::AuthTokens);
struct NoContent;

impl HandleResult for AuthUserResult {
    async fn into_response(self, session: &mut Session, ctx: &AuthContext<'_>) -> PingoraResult<bool> {
        let tokens = match self.tokens {
            Some(t) => t,
            None => {
                tracing::error!("Auth response missing tokens");
                return respond_error(
                    session,
                    500,
                    "Internal server error",
                    ctx.origin,
                    ctx.allowed_origins,
                    ctx.request_id,
                )
                .await;
            }
        };
        let session_dto = match tokens_to_session(&tokens) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("{e}");
                return respond_error(
                    session,
                    500,
                    "Internal server error",
                    ctx.origin,
                    ctx.allowed_origins,
                    ctx.request_id,
                )
                .await;
            }
        };
        let response = AuthResponseDto {
            user: UserDto {
                id: self.user_id,
                email: self.email,
                username: self.username,
                avatar_url: None,
                bio: None,
            },
            session: session_dto,
        };
        respond_json(
            session,
            self.status,
            &response,
            ctx.origin,
            ctx.allowed_origins,
            ctx.request_id,
        )
        .await
    }
}

impl HandleResult for TokensResult {
    async fn into_response(self, session: &mut Session, ctx: &AuthContext<'_>) -> PingoraResult<bool> {
        let session_dto = match tokens_to_session(&self.0) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("{e}");
                return respond_error(
                    session,
                    500,
                    "Internal server error",
                    ctx.origin,
                    ctx.allowed_origins,
                    ctx.request_id,
                )
                .await;
            }
        };
        respond_json(session, 200, &session_dto, ctx.origin, ctx.allowed_origins, ctx.request_id).await
    }
}

impl HandleResult for NoContent {
    async fn into_response(self, session: &mut Session, ctx: &AuthContext<'_>) -> PingoraResult<bool> {
        let mut header = ResponseHeader::build(204, None)?;
        header.insert_header("X-Request-Id", ctx.request_id)?;
        crate::insert_cors_headers(&mut header, ctx.origin, ctx.allowed_origins)?;
        session.write_response_header(Box::new(header), true).await?;
        Ok(true)
    }
}

async fn handle_login(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
) -> PingoraResult<bool> {
    post_grpc_handler(session, auth_client, ctx, |mut client, body: LoginBody| async move {
        let mut req = tonic::Request::new(proto::LoginRequest {
            email: body.email,
            password: body.password,
        });
        req.set_timeout(GRPC_TIMEOUT);
        let r = client.login(req).await?.into_inner();
        Ok(AuthUserResult {
            user_id: r.user_id,
            email: r.email,
            username: r.username,
            tokens: r.tokens,
            status: 200,
        })
    })
    .await
}

async fn handle_register(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
) -> PingoraResult<bool> {
    post_grpc_handler(session, auth_client, ctx, |mut client, body: RegisterBody| async move {
        let mut req = tonic::Request::new(proto::RegisterRequest {
            email: body.email,
            password: body.password,
            username: body.username,
        });
        req.set_timeout(GRPC_TIMEOUT);
        let r = client.register(req).await?.into_inner();
        Ok(AuthUserResult {
            user_id: r.user_id,
            email: r.email,
            username: r.username,
            tokens: r.tokens,
            status: 201,
        })
    })
    .await
}

async fn handle_refresh(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
) -> PingoraResult<bool> {
    post_grpc_handler(session, auth_client, ctx, |mut client, body: RefreshBody| async move {
        let mut req = tonic::Request::new(proto::RefreshTokenRequest {
            refresh_token: body.refresh_token,
        });
        req.set_timeout(GRPC_TIMEOUT);
        Ok(TokensResult(client.refresh_token(req).await?.into_inner()))
    })
    .await
}

async fn handle_logout(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    ctx: &AuthContext<'_>,
) -> PingoraResult<bool> {
    post_grpc_handler(session, auth_client, ctx, |mut client, body: LogoutBody| async move {
        let mut req = tonic::Request::new(proto::LogoutRequest {
            refresh_token: body.refresh_token,
        });
        req.set_timeout(GRPC_TIMEOUT);
        client.logout(req).await?;
        Ok(NoContent)
    })
    .await
}

async fn handle_oauth_start(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    provider: proto::OAuthProvider,
    provider_name: &str,
    config: &Config,
    request_id: &str,
) -> PingoraResult<bool> {
    let mut client = auth_client.clone();
    let redirect_uri = format!("{}?provider={}", config.oauth_callback_url, provider_name);

    let mut req = tonic::Request::new(proto::OAuthGetAuthUrlRequest {
        provider: provider.into(),
        redirect_uri,
    });
    req.set_timeout(GRPC_TIMEOUT);

    match client.o_auth_get_auth_url(req).await {
        Ok(resp) => {
            let url = resp.into_inner().authorize_url;
            respond_redirect(session, &url, request_id).await
        }
        Err(status) => {
            tracing::error!("OAuthGetAuthUrl failed: {}", status);
            let location = format!("{}/?error=oauth_failed", config.frontend_url);
            respond_redirect(session, &location, request_id).await
        }
    }
}

async fn handle_oauth_callback(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    config: &Config,
    request_id: &str,
) -> PingoraResult<bool> {
    let query = session.req_header().uri.query().unwrap_or("");

    let mut code = None;
    let mut provider_str = None;
    for param in query.split('&') {
        if let Some(v) = param.strip_prefix("code=") {
            code = urldecode(v);
        } else if let Some(v) = param.strip_prefix("provider=") {
            provider_str = Some(v.to_string());
        }
    }

    let Some(code) = code else {
        let location = format!("{}/?error=missing_code", config.frontend_url);
        return respond_redirect(session, &location, request_id).await;
    };

    let Some(ref provider_name) = provider_str else {
        let location = format!("{}/?error=unknown_provider", config.frontend_url);
        return respond_redirect(session, &location, request_id).await;
    };

    let provider = match provider_name.as_str() {
        "google" => proto::OAuthProvider::OauthProviderGoogle,
        "github" => proto::OAuthProvider::OauthProviderGithub,
        _ => {
            let location = format!("{}/?error=unknown_provider", config.frontend_url);
            return respond_redirect(session, &location, request_id).await;
        }
    };

    let redirect_uri = format!("{}?provider={}", config.oauth_callback_url, provider_name);

    let mut client = auth_client.clone();
    let mut req = tonic::Request::new(proto::OAuthAuthenticateRequest {
        provider: provider.into(),
        code,
        redirect_uri,
    });
    req.set_timeout(GRPC_TIMEOUT);

    let tokens = match client.o_auth_authenticate(req).await {
        Ok(resp) => resp.into_inner(),
        Err(status) => {
            tracing::error!("OAuthAuthenticate failed: {}", status);
            let location = format!("{}/?error=oauth_auth_failed", config.frontend_url);
            return respond_redirect(session, &location, request_id).await;
        }
    };

    let mut validate_req = tonic::Request::new(proto::ValidateTokenRequest {
        access_token: tokens.access_token.clone(),
    });
    validate_req.set_timeout(GRPC_TIMEOUT);
    let user_info = match client.validate_token(validate_req).await {
        Ok(resp) => Some(resp.into_inner()),
        Err(status) => {
            tracing::warn!("OAuth token validation failed: {status}");
            None
        }
    };

    let session_dto = match tokens_to_session(&tokens) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("{e}");
            let location = format!("{}/?error=internal_error", config.frontend_url);
            return respond_redirect(session, &location, request_id).await;
        }
    };

    let user_id = user_info.as_ref().map(|u| u.user_id.as_str()).unwrap_or("");
    let email = user_info.as_ref().map(|u| u.email.as_str()).unwrap_or("");
    let username = user_info.as_ref().map(|u| u.username.as_str()).unwrap_or("");

    let location = format!(
        "{}/account/oauth-callback#access_token={}&refresh_token={}&expires_in={}&user_id={}&email={}&username={}",
        config.frontend_url,
        urlencode(&session_dto.access_token),
        urlencode(&session_dto.refresh_token),
        session_dto.expires_in,
        urlencode(user_id),
        urlencode(email),
        urlencode(username),
    );

    respond_redirect(session, &location, request_id).await
}
