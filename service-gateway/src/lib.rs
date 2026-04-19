pub mod auth_handler;
pub mod config;

pub mod proto {
    tonic::include_proto!("auth");
}

use config::Config;
use pingora::http::ResponseHeader;
use pingora::prelude::{Error, HTTPStatus, HttpPeer, ProxyHttp, RequestHeader, Session};
use pingora_limits::rate::Rate;
use proto::auth_service_client::AuthServiceClient;
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tokio::sync::OnceCell;
use tonic::transport::{Channel, Endpoint};
use uuid::Uuid;

pub type PingoraResult<T> = pingora::Result<T>;

static RATE_LIMITER: LazyLock<Rate> = LazyLock::new(|| Rate::new(Duration::from_secs(1)));

fn insert_cors_headers(header: &mut ResponseHeader, origin: Option<&str>, allowed_origins: &[String]) -> PingoraResult<()> {
    let allowed = match origin {
        Some(o) if allowed_origins.is_empty() || allowed_origins.iter().any(|a| a == o) => o,
        _ => return Ok(()),
    };
    header.insert_header("Access-Control-Allow-Origin", allowed)?;
    header.insert_header("Access-Control-Allow-Credentials", "true")?;
    header.insert_header("Vary", "Origin")?;
    Ok(())
}

async fn respond_cors_preflight(session: &mut Session, origin: Option<&str>, allowed_origins: &[String]) -> PingoraResult<bool> {
    let mut header = ResponseHeader::build(204, None)?;
    insert_cors_headers(&mut header, origin, allowed_origins)?;
    header.insert_header("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS")?;
    header.insert_header("Access-Control-Allow-Headers", "Content-Type, Authorization")?;
    header.insert_header("Access-Control-Max-Age", "86400")?;
    session.write_response_header(Box::new(header), true).await?;
    Ok(true)
}

pub struct RequestCtx {
    pub bytes_read: usize,
    pub request_id: String,
    pub is_grpc: bool,
    pub origin: Option<String>,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub email: Option<String>,
}

struct Upstream {
    addr: SocketAddr,
    is_grpc: bool,
}

pub struct Gateway {
    pub images_upstream: SocketAddr,
    pub chats_upstream: SocketAddr,
    pub channels_upstream: SocketAddr,
    pub calls_upstream: SocketAddr,
    pub auth_upstream: SocketAddr,
    pub auth_endpoint: Endpoint,
    auth_client: OnceCell<AuthServiceClient<Channel>>,
    pub config: Arc<Config>,
}

impl Gateway {
    pub fn new(
        images_upstream: SocketAddr,
        chats_upstream: SocketAddr,
        channels_upstream: SocketAddr,
        calls_upstream: SocketAddr,
        auth_upstream: SocketAddr,
        auth_endpoint: Endpoint,
        config: Arc<Config>,
    ) -> Self {
        Self {
            images_upstream,
            chats_upstream,
            channels_upstream,
            calls_upstream,
            auth_upstream,
            auth_endpoint,
            auth_client: OnceCell::new(),
            config,
        }
    }

    async fn get_auth_client(&self) -> &AuthServiceClient<Channel> {
        self.auth_client
            .get_or_init(|| async {
                let channel = self.auth_endpoint.connect_lazy();
                AuthServiceClient::new(channel)
            })
            .await
    }
}

fn is_public_route(path: &str) -> bool {
    path.starts_with("/auth.") || path.starts_with("/access/") || path == "/ping" || path == "/health" || path == "/metrics"
}

fn extract_token(session: &Session, path: &str) -> Option<String> {
    if let Some(auth) = session.req_header().headers.get("authorization")
        && let Ok(auth_str) = auth.to_str()
        && let Some(token) = auth_str.strip_prefix("Bearer ")
    {
        return Some(token.to_string());
    }

    if path.starts_with("/ws")
        && let Some(query) = session.req_header().uri.query()
    {
        for param in query.split('&') {
            if let Some(token) = param.strip_prefix("token=") {
                return Some(token.to_string());
            }
        }
    }

    None
}

async fn respond_unauthorized(
    session: &mut Session,
    origin: Option<&str>,
    allowed_origins: &[String],
    request_id: &str,
) -> PingoraResult<bool> {
    let mut header = ResponseHeader::build(401, None)?;
    header.insert_header("Content-Type", "application/json")?;
    header.insert_header("WWW-Authenticate", "Bearer")?;
    header.insert_header("X-Request-Id", request_id)?;
    insert_cors_headers(&mut header, origin, allowed_origins)?;
    session.write_response_header(Box::new(header), false).await?;
    session
        .write_response_body(Some(bytes::Bytes::from(r#"{"error":"Unauthorized"}"#)), true)
        .await?;
    Ok(true)
}

impl Gateway {
    fn rate_limit_key(&self, session: &mut Session) -> String {
        session
            .req_header()
            .headers
            .get("appid")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                session
                    .client_addr()
                    .and_then(|addr| addr.as_inet())
                    .map(|addr| addr.ip().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            })
    }

    fn route_upstream(&self, path: &str) -> PingoraResult<Upstream> {
        match path {
            p if p.starts_with("/images") => Ok(Upstream {
                addr: self.images_upstream,
                is_grpc: false,
            }),
            p if p.starts_with("/ws") => Ok(Upstream {
                addr: self.chats_upstream,
                is_grpc: false,
            }),
            p if p.starts_with("/channels") => Ok(Upstream {
                addr: self.channels_upstream,
                is_grpc: false,
            }),
            p if p.starts_with("/rooms") => Ok(Upstream {
                addr: self.calls_upstream,
                is_grpc: false,
            }),
            p if p.starts_with("/auth.") => Ok(Upstream {
                addr: self.auth_upstream,
                is_grpc: true,
            }),
            p if p.starts_with("/access/") => {
                tracing::warn!(path = %p, "Unexpected /access/ route reached upstream routing");
                Err(Error::explain(HTTPStatus(500), "Auth route not intercepted"))
            }
            _ => {
                tracing::warn!(path = %path, "Unknown path");
                Err(Error::explain(HTTPStatus(404), "Not Found"))
            }
        }
    }
}

#[async_trait::async_trait]
impl ProxyHttp for Gateway {
    type CTX = RequestCtx;

    fn new_ctx(&self) -> Self::CTX {
        RequestCtx {
            bytes_read: 0,
            request_id: Uuid::now_v7().to_string(),
            is_grpc: false,
            origin: None,
            user_id: None,
            username: None,
            email: None,
        }
    }

    async fn upstream_peer(&self, session: &mut Session, ctx: &mut Self::CTX) -> PingoraResult<Box<HttpPeer>> {
        let path = session.req_header().uri.path();
        let route = self.route_upstream(path)?;
        ctx.is_grpc = route.is_grpc;

        let mut peer = HttpPeer::new(route.addr, false, "".into());
        peer.options.connection_timeout = Some(Duration::from_secs(self.config.connection_timeout_secs));
        peer.options.total_connection_timeout = Some(Duration::from_secs(self.config.total_connection_timeout_secs));
        peer.options.read_timeout = Some(Duration::from_secs(self.config.read_timeout_secs));
        peer.options.write_timeout = Some(Duration::from_secs(self.config.write_timeout_secs));

        if route.is_grpc {
            peer.options.alpn = pingora::protocols::ALPN::H2;
        }

        Ok(Box::new(peer))
    }

    async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) -> PingoraResult<bool> {
        if let Some(value) = session.req_header().headers.get("Content-Length")
            && let Ok(len_str) = value.to_str()
            && let Ok(len) = len_str.parse::<usize>()
            && len > self.config.max_body_size
        {
            tracing::warn!("Rejecting request: Content-Length {} > {}", len, self.config.max_body_size);
            session.respond_error(413).await?;
            return Ok(true);
        }

        let key = self.rate_limit_key(session);
        let curr_window_requests = RATE_LIMITER.observe(&key, 1);
        if curr_window_requests > self.config.max_req_per_sec {
            let mut header = ResponseHeader::build(429, None)?;
            header.insert_header("X-Rate-Limit-Limit", self.config.max_req_per_sec.to_string())?;
            header.insert_header("X-Rate-Limit-Remaining", "0")?;
            header.insert_header("X-Rate-Limit-Reset", "1")?;
            session.set_keepalive(None);
            session.write_response_header(Box::new(header), true).await?;
            return Ok(true);
        }

        ctx.origin = session
            .req_header()
            .headers
            .get("origin")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let path = session.req_header().uri.path();
        let method = session.req_header().method.as_str();

        if method == "OPTIONS" {
            return respond_cors_preflight(session, ctx.origin.as_deref(), &self.config.allowed_origins).await;
        }

        if path.starts_with("/access/") {
            let path = path.to_string();
            let method = method.to_string();
            let auth_ctx = auth_handler::AuthContext {
                origin: ctx.origin.as_deref(),
                allowed_origins: &self.config.allowed_origins,
                request_id: &ctx.request_id,
                max_body_size: self.config.max_body_size,
                config: &self.config,
            };
            return auth_handler::handle_auth_route(session, &path, &method, self.get_auth_client().await, &auth_ctx).await;
        }

        if !is_public_route(path) {
            let Some(token) = extract_token(session, path) else {
                return respond_unauthorized(session, ctx.origin.as_deref(), &self.config.allowed_origins, &ctx.request_id).await;
            };

            let mut client = self.get_auth_client().await.clone();
            match client
                .validate_token(proto::ValidateTokenRequest { access_token: token })
                .await
            {
                Ok(response) => {
                    let resp = response.into_inner();
                    ctx.user_id = Some(resp.user_id);
                    ctx.username = Some(resp.username);
                    ctx.email = Some(resp.email);
                }
                Err(status) => {
                    tracing::warn!("Token validation failed: {}", status);
                    return respond_unauthorized(session, ctx.origin.as_deref(), &self.config.allowed_origins, &ctx.request_id)
                        .await;
                }
            }
        }

        Ok(false)
    }

    async fn request_body_filter(
        &self,
        _session: &mut Session,
        body: &mut Option<bytes::Bytes>,
        _end_of_stream: bool,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()>
    where
        Self::CTX: Send + Sync,
    {
        if let Some(b) = body {
            ctx.bytes_read += b.len();
            if ctx.bytes_read > self.config.max_body_size {
                tracing::warn!(
                    "Rejecting request: accumulated {} bytes > {}",
                    ctx.bytes_read,
                    self.config.max_body_size
                );
                return Err(Error::explain(HTTPStatus(413), "Stream exceeded limit"));
            }
        }
        Ok(())
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()> {
        let client_addr = session
            .client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        upstream_request.remove_header("X-User-Id");
        upstream_request.remove_header("X-Username");
        upstream_request.remove_header("X-Email");
        upstream_request.remove_header("X-Request-Id");

        let proto = session
            .req_header()
            .headers
            .get("X-Forwarded-Proto")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("http");

        upstream_request.insert_header("X-Forwarded-For", &client_addr)?;
        upstream_request.insert_header("X-Real-IP", &client_addr)?;
        upstream_request.insert_header("X-Forwarded-Proto", proto)?;
        upstream_request.insert_header("X-Request-Id", &ctx.request_id)?;

        if let Some(ref user_id) = ctx.user_id {
            upstream_request.insert_header("X-User-Id", user_id)?;
        }
        if let Some(ref username) = ctx.username {
            upstream_request.insert_header("X-Username", username)?;
        }
        if let Some(ref email) = ctx.email {
            upstream_request.insert_header("X-Email", email)?;
        }

        let host_value = upstream_request
            .headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        if let Some(host_str) = host_value {
            upstream_request.insert_header("X-Forwarded-Host", &host_str)?;
        }

        Ok(())
    }

    async fn upstream_response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> PingoraResult<()> {
        insert_cors_headers(upstream_response, ctx.origin.as_deref(), &self.config.allowed_origins)?;
        upstream_response.insert_header("X-Request-Id", &ctx.request_id)?;
        Ok(())
    }

    async fn logging(&self, session: &mut Session, e: Option<&Error>, ctx: &mut Self::CTX) {
        let method = session.req_header().method.as_str();
        let path = session.req_header().uri.path();
        let status = session.response_written().map(|r| r.status.as_u16()).unwrap_or(0);
        let client_addr = session.client_addr().map(|a| a.to_string()).unwrap_or_default();

        if let Some(error) = e {
            tracing::error!(
                request_id = %ctx.request_id,
                method = %method,
                path = %path,
                status = status,
                client = %client_addr,
                error = %error,
                "Request failed"
            );
        } else {
            tracing::info!(
                request_id = %ctx.request_id,
                method = %method,
                path = %path,
                status = status,
                client = %client_addr,
                "Request completed"
            );
        }
    }

    fn fail_to_connect(&self, _session: &mut Session, _peer: &HttpPeer, _ctx: &mut Self::CTX, e: Box<Error>) -> Box<Error> {
        tracing::error!(error = %e, "Failed to connect to upstream");
        Error::explain(HTTPStatus(502), "Bad Gateway")
    }

    fn error_while_proxy(
        &self,
        _peer: &HttpPeer,
        _session: &mut Session,
        e: Box<Error>,
        _ctx: &mut Self::CTX,
        _client_reused: bool,
    ) -> Box<Error> {
        tracing::error!(error = %e, "Error while proxying");
        e
    }
}

pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .init();
}

pub fn parse_upstream(addr: &str) -> SocketAddr {
    addr.parse().unwrap_or_else(|_| panic!("Invalid upstream address: {addr}"))
}

pub fn log_config(config: &Config) {
    tracing::info!("--- Gateway configuration ---");
    tracing::info!("listen: {}", config.listen_addr);
    tracing::info!("images upstream: {}", config.images_upstream);
    tracing::info!("chats upstream: {}", config.chats_upstream);
    tracing::info!("channels upstream: {}", config.channels_upstream);
    tracing::info!("calls upstream: {}", config.calls_upstream);
    tracing::info!("auth upstream (gRPC): {}", config.auth_upstream);
    tracing::info!("max req/sec: {}", config.max_req_per_sec);
    tracing::info!("max body size: {} bytes", config.max_body_size);
    tracing::info!("connection timeout: {}s", config.connection_timeout_secs);
    tracing::info!("total connection timeout: {}s", config.total_connection_timeout_secs);
    tracing::info!("read timeout: {}s", config.read_timeout_secs);
    tracing::info!("write timeout: {}s", config.write_timeout_secs);
    tracing::info!("-----------------------------");
}
