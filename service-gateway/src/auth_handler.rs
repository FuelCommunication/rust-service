use std::time::Duration;

use bytes::BytesMut;
use pingora::http::ResponseHeader;
use pingora::prelude::{HTTPStatus, Session};
use serde::{Deserialize, Serialize};
use tonic::transport::Channel;

use crate::config::Config;
use crate::proto::auth_service_client::AuthServiceClient;
use crate::proto;
use crate::PingoraResult;

const GRPC_TIMEOUT: Duration = Duration::from_secs(10);

fn urldecode(s: &str) -> String {
    let mut result = Vec::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(0);
            let lo = chars.next().unwrap_or(0);
            if let (Some(h), Some(l)) = (hex_val(hi), hex_val(lo)) {
                result.push(h << 4 | l);
            }
        } else if b == b'+' {
            result.push(b' ');
        } else {
            result.push(b);
        }
    }
    String::from_utf8(result).unwrap_or_default()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ── Request DTOs ──

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

// ─��� Response DTOs ──

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

// ── Helpers ──

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
    let json = serde_json::to_vec(body).map_err(|e| {
        pingora::Error::explain(HTTPStatus(500), format!("JSON serialization error: {e}"))
    })?;

    let mut header = ResponseHeader::build(status, None)?;
    header.insert_header("Content-Type", "application/json")?;
    header.insert_header("Content-Length", json.len().to_string())?;
    header.insert_header("X-Request-Id", request_id)?;
    crate::insert_cors_headers(&mut header, origin, allowed_origins)?;
    session
        .write_response_header(Box::new(header), false)
        .await?;
    session
        .write_response_body(Some(bytes::Bytes::from(json)), true)
        .await?;
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

async fn respond_redirect(
    session: &mut Session,
    location: &str,
    request_id: &str,
) -> PingoraResult<bool> {
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

fn tokens_to_session(tokens: &proto::AuthTokens) -> SessionDto {
    let now = chrono::Utc::now().timestamp();
    let access_expires_at = tokens.access_expires_at.as_ref().map(|t| t.seconds).unwrap_or(0);
    SessionDto {
        access_token: tokens.access_token.clone(),
        token_type: "Bearer".to_string(),
        refresh_token: tokens.refresh_token.clone(),
        expires_in: access_expires_at - now,
    }
}

// ── Route dispatch ──

pub async fn handle_auth_route(
    session: &mut Session,
    path: &str,
    method: &str,
    auth_client: &AuthServiceClient<Channel>,
    max_body_size: usize,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
    config: &Config,
) -> PingoraResult<bool> {
    // OAuth routes (GET)
    if method == "GET" {
        return match path {
            "/access/oauth/google" => {
                handle_oauth_start(session, auth_client, proto::OAuthProvider::OauthProviderGoogle, "google", config, request_id).await
            }
            "/access/oauth/github" => {
                handle_oauth_start(session, auth_client, proto::OAuthProvider::OauthProviderGithub, "github", config, request_id).await
            }
            "/access/oauth/callback" => {
                handle_oauth_callback(session, auth_client, config, request_id).await
            }
            _ => respond_error(session, 405, "Method not allowed", origin, allowed_origins, request_id).await,
        };
    }

    if method != "POST" {
        return respond_error(session, 405, "Method not allowed", origin, allowed_origins, request_id).await;
    }

    let content_type = session
        .req_header()
        .headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !content_type.starts_with("application/json") {
        return respond_error(session, 415, "Content-Type must be application/json", origin, allowed_origins, request_id).await;
    }

    match path {
        "/access/login" => handle_login(session, auth_client, max_body_size, origin, allowed_origins, request_id).await,
        "/access/register" => handle_register(session, auth_client, max_body_size, origin, allowed_origins, request_id).await,
        "/access/refresh" => handle_refresh(session, auth_client, max_body_size, origin, allowed_origins, request_id).await,
        "/access/logout" => handle_logout(session, auth_client, max_body_size, origin, allowed_origins, request_id).await,
        _ => respond_error(session, 404, "Not found", origin, allowed_origins, request_id).await,
    }
}

// ── Route handlers ──

async fn handle_login(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    max_body_size: usize,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let body = match read_full_body(session, max_body_size).await {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &e, origin, allowed_origins, request_id).await,
    };

    let login_body: LoginBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &format!("Invalid JSON: {e}"), origin, allowed_origins, request_id).await,
    };

    let mut client = auth_client.clone();

    let mut req = tonic::Request::new(proto::LoginRequest {
        email: login_body.email,
        password: login_body.password,
    });
    req.set_timeout(GRPC_TIMEOUT);
    let login_resp = match client.login(req).await {
        Ok(resp) => resp.into_inner(),
        Err(status) => {
            let http_code = grpc_status_to_http(status.code());
            return respond_error(session, http_code, status.message(), origin, allowed_origins, request_id).await;
        }
    };

    let tokens = match login_resp.tokens {
        Some(t) => t,
        None => {
            tracing::error!("Login response missing tokens");
            return respond_error(session, 500, "Internal server error", origin, allowed_origins, request_id).await;
        }
    };

    let response = AuthResponseDto {
        user: UserDto {
            id: login_resp.user_id,
            email: login_resp.email,
            username: login_resp.username,
            avatar_url: None,
            bio: None,
        },
        session: tokens_to_session(&tokens),
    };

    respond_json(session, 200, &response, origin, allowed_origins, request_id).await
}

async fn handle_register(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    max_body_size: usize,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let body = match read_full_body(session, max_body_size).await {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &e, origin, allowed_origins, request_id).await,
    };

    let register_body: RegisterBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &format!("Invalid JSON: {e}"), origin, allowed_origins, request_id).await,
    };

    let mut client = auth_client.clone();

    let mut req = tonic::Request::new(proto::RegisterRequest {
        email: register_body.email,
        password: register_body.password,
        username: register_body.username,
    });
    req.set_timeout(GRPC_TIMEOUT);
    let register_resp = match client.register(req).await {
        Ok(resp) => resp.into_inner(),
        Err(status) => {
            let http_code = grpc_status_to_http(status.code());
            return respond_error(session, http_code, status.message(), origin, allowed_origins, request_id).await;
        }
    };

    let tokens = match register_resp.tokens {
        Some(t) => t,
        None => {
            tracing::error!("Register response missing tokens");
            return respond_error(session, 500, "Internal server error", origin, allowed_origins, request_id).await;
        }
    };

    let response = AuthResponseDto {
        user: UserDto {
            id: register_resp.user_id,
            email: register_resp.email,
            username: register_resp.username,
            avatar_url: None,
            bio: None,
        },
        session: tokens_to_session(&tokens),
    };

    respond_json(session, 201, &response, origin, allowed_origins, request_id).await
}

async fn handle_refresh(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    max_body_size: usize,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let body = match read_full_body(session, max_body_size).await {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &e, origin, allowed_origins, request_id).await,
    };

    let refresh_body: RefreshBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &format!("Invalid JSON: {e}"), origin, allowed_origins, request_id).await,
    };

    let mut client = auth_client.clone();

    let mut req = tonic::Request::new(proto::RefreshTokenRequest {
        refresh_token: refresh_body.refresh_token,
    });
    req.set_timeout(GRPC_TIMEOUT);
    let tokens = match client.refresh_token(req).await {
        Ok(resp) => resp.into_inner(),
        Err(status) => {
            let http_code = grpc_status_to_http(status.code());
            return respond_error(session, http_code, status.message(), origin, allowed_origins, request_id).await;
        }
    };

    respond_json(session, 200, &tokens_to_session(&tokens), origin, allowed_origins, request_id).await
}

async fn handle_logout(
    session: &mut Session,
    auth_client: &AuthServiceClient<Channel>,
    max_body_size: usize,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let body = match read_full_body(session, max_body_size).await {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &e, origin, allowed_origins, request_id).await,
    };

    let logout_body: LogoutBody = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return respond_error(session, 400, &format!("Invalid JSON: {e}"), origin, allowed_origins, request_id).await,
    };

    let mut client = auth_client.clone();

    let mut req = tonic::Request::new(proto::LogoutRequest {
        refresh_token: logout_body.refresh_token,
    });
    req.set_timeout(GRPC_TIMEOUT);
    match client.logout(req).await {
        Ok(_) => {
            let mut header = ResponseHeader::build(204, None)?;
            header.insert_header("X-Request-Id", request_id)?;
            crate::insert_cors_headers(&mut header, origin, allowed_origins)?;
            session.write_response_header(Box::new(header), true).await?;
            Ok(true)
        }
        Err(status) => {
            let http_code = grpc_status_to_http(status.code());
            respond_error(session, http_code, status.message(), origin, allowed_origins, request_id).await
        }
    }
}

// ── OAuth handlers ──

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
            code = Some(urldecode(v));
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

    // Get user info via ValidateToken
    let mut validate_req = tonic::Request::new(proto::ValidateTokenRequest {
        access_token: tokens.access_token.clone(),
    });
    validate_req.set_timeout(GRPC_TIMEOUT);
    let user_info = client.validate_token(validate_req).await.ok().map(|r| r.into_inner());

    let session_dto = tokens_to_session(&tokens);
    let user_id = user_info.as_ref().map(|u| u.user_id.as_str()).unwrap_or("");
    let email = user_info.as_ref().map(|u| u.email.as_str()).unwrap_or("");
    let username = user_info.as_ref().map(|u| u.username.as_str()).unwrap_or("");

    let location = format!(
        "{}/account/oauth-callback#access_token={}&refresh_token={}&expires_in={}&user_id={}&email={}&username={}",
        config.frontend_url,
        session_dto.access_token,
        session_dto.refresh_token,
        session_dto.expires_in,
        user_id,
        email,
        username,
    );

    respond_redirect(session, &location, request_id).await
}
