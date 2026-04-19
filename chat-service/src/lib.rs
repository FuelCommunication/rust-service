mod api;
mod api_response;

use crate::api::{not_found, ping};
use axum::{Router, routing};
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

pub struct ServerBuilder {
    tcp_listener: TcpListener,
    router: Router,
}

impl ServerBuilder {
    pub async fn new() -> Self {
        let tcp_listener = Self::init_tcp_listener().await;
        let router = Self::init_router();

        Self {
            tcp_listener,
            router,
        }
    }

    async fn init_tcp_listener() -> TcpListener {
        let host = std::env::var("HOST").expect("Host don`t set");
        let port = std::env::var("PORT").expect("Port don`t set");
        let addr = format!("{host}:{port}");

        TcpListener::bind(addr).await.expect("the address is busy")
    }

    fn init_router() -> Router {
        Router::new()
            .route("/ping", routing::get(ping))
            .fallback(not_found)
            .layer((
                TraceLayer::new_for_http(),
                TimeoutLayer::new(Duration::from_secs(10)),
            ))
    }

    pub fn init_cors(mut self) -> Self {
        use axum::http::{
            HeaderValue, Method,
            header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, ORIGIN},
        };
        use tower_http::cors::CorsLayer;

        let origins = std::env::var("ORIGINS")
            .expect("ORIGINS env var is not set")
            .split(',')
            .map(|s| s.trim())
            .map(|s| HeaderValue::from_str(s).expect("Invalid origin in ORIGINS"))
            .collect::<Vec<_>>();

        let cors = CorsLayer::new()
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
            ])
            .allow_headers([ORIGIN, AUTHORIZATION, ACCEPT, CONTENT_TYPE])
            .allow_origin(origins);

        self.router = self.router.layer(cors);
        self
    }

    pub fn init_tracing(self) -> Self {
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .compact()
            .with_file(true)
            .with_line_number(true)
            .with_target(false)
            .init();

        self
    }

    pub async fn run(self) {
        tracing::info!(
            "listening on http{}",
            self.tcp_listener.local_addr().unwrap()
        );

        axum::serve(self.tcp_listener, self.router)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .unwrap()
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
