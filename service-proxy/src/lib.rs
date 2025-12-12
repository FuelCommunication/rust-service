use pingora::prelude::{Error, HTTPStatus, HttpPeer, ProxyHttp, RequestHeader, Session};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

pub type ProxyResult<T> = pingora::Result<T>;

pub struct ProxyService {
    python_backend: (&'static str, u16),
    rust_backend: (&'static str, u16),
}

impl ProxyService {
    pub const fn new() -> Self {
        Self {
            python_backend: ("127.0.0.1", 3002),
            rust_backend: ("127.0.0.1", 3000),
        }
    }
}

#[async_trait::async_trait]
impl ProxyHttp for ProxyService {
    type CTX = ();

    fn new_ctx(&self) -> Self::CTX {}

    async fn upstream_peer(&self, session: &mut Session, _ctx: &mut ()) -> ProxyResult<Box<HttpPeer>> {
        let host = session
            .req_header()
            .headers
            .get("host")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.split(":").next())
            .unwrap_or("");

        let addr = match host {
            "rust.localhost" => self.rust_backend,
            "python.localhost" => self.python_backend,
            _ => {
                tracing::warn!(host = %host, "Unknown host");
                return Err(Error::explain(HTTPStatus(404), "Unknown host"));
            }
        };

        let mut peer = HttpPeer::new(addr, false, "".into());
        peer.options.connection_timeout = Some(Duration::from_secs(5));
        peer.options.total_connection_timeout = Some(Duration::from_secs(10));

        Ok(Box::new(peer))
    }

    async fn upstream_request_filter(
        &self,
        session: &mut Session,
        upstream_request: &mut RequestHeader,
        _ctx: &mut Self::CTX,
    ) -> pingora::Result<()> {
        let client_addr = session
            .client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        upstream_request.insert_header("X-Forwarded-For", &client_addr)?;
        upstream_request.insert_header("X-Real-IP", &client_addr)?;

        let scheme = if session.digest().is_some() { "https" } else { "http" };
        upstream_request.insert_header("X-Forwarded-Proto", scheme)?;

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

    async fn logging(&self, session: &mut Session, _e: Option<&Error>, _ctx: &mut Self::CTX) {
        let method = session.req_header().method.as_str();
        let path = session.req_header().uri.path();
        let status = session.response_written().map(|r| r.status.as_u16()).unwrap_or(0);

        tracing::info!(
            method = %method,
            path = %path,
            status = status,
            "Request completed"
        );
    }

    fn fail_to_connect(&self, _session: &mut Session, _peer: &HttpPeer, _ctx: &mut Self::CTX, e: Box<Error>) -> Box<Error> {
        tracing::error!(error = %e, "Failed to connect to upstream");
        Error::explain(HTTPStatus(502), format!("Bad Gateway: {}", e))
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

impl Default for ProxyService {
    fn default() -> Self {
        Self::new()
    }
}

pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_target(false)
        .init();
}
