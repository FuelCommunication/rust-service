mod api;
pub mod config;
pub mod error;
pub mod events;
pub mod state;

use api::{health, not_found, ping, router::websocket_handler, schemas::ServerEvent};
use axum::{Router, http::StatusCode, routing};
pub use config::Config;
use events::ChannelEvent;
use futures_util::StreamExt;
use mimalloc::MiMalloc;
use state::ServerState;
use std::time::Duration;
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowHeaders, AllowMethods},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use uuid::Uuid;
use kafka_client::{
    config::ConsumerConfig,
    consumer::KafkaConsumer
};

use crate::state::ServerData;

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
        let state = ServerData::new(&config).await;
        let router = Self::init_router(state.clone());

        Self::spawn_kafka_consumer(&config, state);

        Self {
            tcp_listener,
            router,
            config,
        }
    }

    fn spawn_kafka_consumer(config: &Config, state: ServerState) {
        let consumer_config = ConsumerConfig::builder(&config.kafka_brokers, &config.kafka_group_id, &config.kafka_topic)
            .build()
            .expect("Invalid Kafka consumer config");
        let consumer = KafkaConsumer::new(consumer_config).expect("Failed to create Kafka consumer");

        tokio::spawn(async move {
            let stream = consumer.stream::<ChannelEvent>();
            tokio::pin!(stream);
            while let Some(result) = stream.next().await {
                let event = match result {
                    Ok(event) => event,
                    Err(e) => {
                        tracing::error!("Failed to consume Kafka event: {e}");
                        continue;
                    }
                };

                match event {
                    ChannelEvent::ChannelDeleted { channel_id } => {
                        if let Some(room) = state.rooms.get(&channel_id) {
                            let _ = room.sender.send(ServerEvent::ChannelDeleted);
                        }
                        state.rooms.remove(&channel_id);
                        tracing::info!("Channel {channel_id} deleted — room removed");
                    }
                    ChannelEvent::UserUnsubscribed { channel_id, user_id } => {
                        if let Ok(uid) = user_id.parse::<Uuid>() {
                            if let Some(room) = state.rooms.get(&channel_id) {
                                let _ = room.sender.send(ServerEvent::Kicked { user_id: uid });
                            }
                            tracing::info!("User {user_id} unsubscribed from channel {channel_id}");
                        }
                    }
                    ChannelEvent::ChannelUpdated { .. } | ChannelEvent::UserSubscribed { .. } => {}
                }
            }
            tracing::warn!("Kafka consumer stream ended");
        });
    }

    async fn init_tcp_listener(config: &Config) -> TcpListener {
        let addr = format!("{}:{}", config.host, config.port);
        TcpListener::bind(addr).await.expect("the address is busy")
    }

    pub fn init_router(state: ServerState) -> Router {
        Router::new()
            .route("/ping", routing::get(ping))
            .route("/health", routing::get(health))
            .fallback(not_found)
            .route_layer(TimeoutLayer::with_status_code(
                StatusCode::REQUEST_TIMEOUT,
                Duration::from_secs(10),
            ))
            .route("/ws/{room}", routing::get(websocket_handler))
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
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            tracing::info!("Received terminate signal");
        },
    }

    tracing::info!("Starting graceful shutdown");
}
