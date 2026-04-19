pub mod config;
pub mod room;
pub mod routes;
pub mod signal;
pub mod state;
pub mod ws;

use std::time::Duration;

use axum::{Router, http::StatusCode, routing};
use axum_prometheus::PrometheusMetricLayer;
use mimalloc::MiMalloc;
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowHeaders, AllowMethods},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::state::{ServerData, ServerState};

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
        let state = ServerData::new(
            config.max_peers_per_room,
            config.room_idle_timeout_secs,
            config.max_message_size,
            config.channel_capacity,
            config.heartbeat_interval_secs,
        );
        let router = Self::init_router(state, config.request_timeout_secs);

        Self {
            tcp_listener,
            router,
            config,
        }
    }

    async fn init_tcp_listener(config: &Config) -> TcpListener {
        let addr = format!("{}:{}", config.host, config.port);
        TcpListener::bind(&addr).await.expect("the address is busy")
    }

    fn init_router(state: ServerState, request_timeout_secs: u64) -> Router {
        Router::new()
            .route("/ping", routing::get(ping))
            .route("/rooms", routing::get(routes::list_rooms).post(routes::create_room))
            .route("/rooms/{room_id}", routing::get(routes::get_room).delete(routes::delete_room))
            .fallback(not_found)
            .route_layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(request_timeout_secs),
            ))
            .route("/rooms/ws/{room_id}", routing::get(ws::ws_handler))
            .with_state(state)
            .layer(TraceLayer::new_for_http())
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

async fn ping() -> &'static str {
    "pong"
}

async fn not_found() -> StatusCode {
    StatusCode::NOT_FOUND
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
