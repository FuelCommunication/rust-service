mod api;
pub mod config;
pub mod error;
pub mod state;

use api::{
    not_found, ping,
    router::{delete_image, download_image, upload_image},
};
use axum::{Router, http::StatusCode, routing};
use config::Config;
use mimalloc::MiMalloc;
use state::ServerState;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowHeaders, AllowMethods},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub struct ServerBuilder {
    tcp_listener: TcpListener,
    router: Router,
    config: Config,
}

impl ServerBuilder {
    pub async fn new(config: Config) -> Self {
        let tcp_listener = Self::init_tcp_listener(&config).await;
        let state = state::ServerData::new(&config).await;
        let router = Self::init_router(state).layer((
            TraceLayer::new_for_http(),
            TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(10)),
        ));

        Self {
            tcp_listener,
            router,
            config,
        }
    }

    async fn init_tcp_listener(config: &Config) -> TcpListener {
        let addr = format!("{}:{}", config.host, config.port);
        TcpListener::bind(addr).await.expect("the address is busy")
    }

    pub fn init_router(state: ServerState) -> Router {
        Router::new()
            .route("/ping", routing::get(ping))
            .route("/images/upload", routing::post(upload_image))
            .route("/images/{filename}", routing::get(download_image).delete(delete_image))
            .with_state(state)
            .fallback(not_found)
    }

    pub fn with_cors<M: Into<AllowMethods>, H: Into<AllowHeaders>>(mut self, methods: M, headers: H) -> Self {
        use axum::http::HeaderValue;
        use tower_http::cors::CorsLayer;

        let origins = self
            .config
            .origins
            .split(',')
            .map(|s| s.trim())
            .map(|s| HeaderValue::from_str(s).expect("Invalid origin in ORIGINS"))
            .collect::<Vec<_>>();

        let cors = CorsLayer::new()
            .allow_methods(methods)
            .allow_headers(headers)
            .allow_origin(origins);

        self.router = self.router.layer(cors);
        self
    }

    pub fn with_tracing(self) -> Self {
        use tracing_subscriber::EnvFilter;

        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .compact()
            .with_file(true)
            .with_line_number(true)
            .with_target(false)
            .init();

        self
    }

    pub fn with_prometheus(mut self) -> Self {
        use axum_prometheus::PrometheusMetricLayer;

        let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();
        self.router = self
            .router
            .route("/metrics", routing::get(|| async move { metric_handle.render() }))
            .layer(prometheus_layer);

        self
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("listening on http://{}", self.tcp_listener.local_addr()?);

        axum::serve(self.tcp_listener, self.router)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        tracing::info!("Graceful shutdown complete");
        Ok(())
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received Ctrl+C signal"),
        _ = terminate => tracing::info!("Received terminate signal"),
    }

    tracing::info!("Starting graceful shutdown");
}
