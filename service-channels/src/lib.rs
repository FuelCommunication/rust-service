pub mod config;
pub mod error;
pub mod events;
pub mod models;
pub mod routes;
pub mod schemas;
pub mod search;
pub mod state;
pub mod store;

use std::{sync::Arc, time::Duration};

use axum::{Router, http::StatusCode, routing};
use axum_prometheus::PrometheusMetricLayer;
use mimalloc::MiMalloc;
use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowHeaders, AllowMethods},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;
use valkey_client::Valkey;

use kafka_client::config::ProducerConfig;
use kafka_client::producer::KafkaProducer;

use crate::{
    config::Config,
    search::SearchService,
    state::{ServerData, ServerState},
    store::ChannelStore,
};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

pub struct ServerBuilder {
    tcp_listener: TcpListener,
    router: Router,
    state: ServerState,
    config: Config,
}

impl ServerBuilder {
    pub async fn new(config: Config) -> Self {
        let tcp_listener = Self::init_tcp_listener(&config).await;
        let state = Self::init_state(&config).await;
        let router = Self::init_router(state.clone()).layer((
            TraceLayer::new_for_http(),
            TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(10)),
        ));
        Self {
            tcp_listener,
            router,
            state,
            config,
        }
    }

    async fn init_tcp_listener(config: &Config) -> TcpListener {
        let addr = format!("{}:{}", config.host, config.port);
        TcpListener::bind(&addr).await.expect("the address is busy")
    }

    async fn init_state(config: &Config) -> ServerState {
        let pool = PgPoolOptions::new()
            .max_connections(config.db_max_connections)
            .min_connections(config.db_min_connections)
            .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
            .connect(&config.database_url)
            .await
            .expect("Failed to connect to PostgreSQL");

        let cache = Valkey::new(&config.valkey_url).await.expect("Failed to connect to Valkey");

        let search = SearchService::new(&config.meilisearch_url, config.meilisearch_api_key.as_deref())
            .await
            .expect("Failed to initialize Meilisearch");

        let kafka_config = ProducerConfig::builder(&config.kafka_brokers, &config.kafka_topic)
            .auto_create_topics(true)
            .build()
            .expect("Invalid Kafka producer config");
        let kafka = KafkaProducer::new(kafka_config).expect("Failed to create Kafka producer");

        let store = ChannelStore::new(pool);
        Arc::new(ServerData {
            store,
            cache,
            cache_ttl: config.cache_ttl_secs,
            search,
            producer: kafka,
        })
    }

    fn init_router(state: ServerState) -> Router {
        Router::new()
            .route("/ping", routing::get(ping))
            .route("/channels", routing::get(routes::get_channels).post(routes::create_channel))
            .route("/channels/search", routing::get(routes::search_channels))
            .route(
                "/channels/{channel_id}",
                routing::get(routes::get_channel)
                    .patch(routes::update_channel)
                    .delete(routes::delete_channel),
            )
            .route("/channels/sub/user/{user_id}", routing::get(routes::get_user_subscriptions))
            .route(
                "/channels/sub/channel/{channel_id}",
                routing::get(routes::get_channel_subscribers),
            )
            .route(
                "/channels/{channel_id}/subscribers/check",
                routing::get(routes::check_subscription),
            )
            .route(
                "/channels/{channel_id}/subscribe",
                routing::post(routes::subscribe).delete(routes::unsubscribe),
            )
            .route(
                "/channels/{channel_id}/transfer/{user_id}",
                routing::post(routes::transfer_ownership),
            )
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

        self.state.store.pool().close().await;
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
