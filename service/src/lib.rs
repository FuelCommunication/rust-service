mod api;
mod error;
mod state;

use api::{
    chats::router::websocket_handler,
    images::router::{delete_image, download_image, upload_image},
    not_found, ping,
};
use axum::{Router, extract::DefaultBodyLimit, routing};
use kafka::{
    config::{ConsumerConfig, LogLevel, ProducerConfig},
    consumer::KafkaConsumer,
    producer::KafkaProducer,
};
use s3::S3;
use state::{KafkaState, ServerData, ServerState};
use std::{sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tower_http::{
    cors::{AllowHeaders, AllowMethods},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing_subscriber::EnvFilter;

pub struct ServerBuilder {
    tcp_listener: TcpListener,
    router: Router,
}

impl ServerBuilder {
    pub async fn new() -> Self {
        let tcp_listener = Self::init_tcp_listener().await;
        let router = Self::init_router().await;

        Self { tcp_listener, router }
    }

    pub async fn init_tcp_listener() -> TcpListener {
        let host = read_env_var("HOST");
        let port = read_env_var("PORT");
        let addr = format!("{host}:{port}");

        TcpListener::bind(addr).await.expect("the address is busy")
    }

    pub async fn init_router() -> Router {
        let state = Self::init_state().await;

        Router::new()
            .route("/ping", routing::get(ping))
            .route("/images/upload/{user_id}", routing::post(upload_image))
            .route("/images/{filename}", routing::get(download_image).delete(delete_image))
            .route("/ws/{room}", routing::get(websocket_handler))
            .with_state(state)
            .fallback(not_found)
            .layer((
                TraceLayer::new_for_http(),
                TimeoutLayer::new(Duration::from_secs(10)),
                DefaultBodyLimit::max(2 * 1024 * 1024),
            ))
    }

    pub fn with_cors<M: Into<AllowMethods>, H: Into<AllowHeaders>>(mut self, methods: M, headers: H) -> Self {
        use axum::http::HeaderValue;
        use tower_http::cors::CorsLayer;

        let origins = read_env_var("ORIGINS")
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

    async fn init_state() -> ServerState {
        let access_key = read_env_var("ACCESS_KEY");
        let secret_key = read_env_var("SECRET_KEY");
        let region = read_env_var("REGION");
        let endpoint_url = read_env_var("ENDPOINT_URL");
        let bucket: &'static str = Box::leak(read_env_var("BUCKET").into_boxed_str());
        let s3 = S3::new(access_key, secret_key, region, endpoint_url, bucket).await;

        let brokers = read_env_var("BROKERS");
        let topic = read_env_var("TOPIC");
        let group_id = read_env_var("GROUP_ID");
        let producer_config = ProducerConfig::new(&brokers, &topic).expect("Invalid producer config");
        let consumer_config = ConsumerConfig::new(brokers, group_id, topic, LogLevel::Debug).expect("Invalid consumer config");
        let producer = KafkaProducer::new(producer_config).unwrap();
        let consumer = KafkaConsumer::new(consumer_config).unwrap();
        let kafka = KafkaState { producer, consumer };

        Arc::new(ServerData { s3, kafka })
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

    pub async fn run(self) {
        tracing::info!("listening on http{}", self.tcp_listener.local_addr().unwrap());

        axum::serve(self.tcp_listener, self.router)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .unwrap()
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} don`t set"))
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
