mod api;
mod error;
mod state;

use api::{
    images::router::{delete_image, download_image, upload_image},
    not_found, ping,
};
use axum::{Router, routing};
use kafka::{
    config::{ConsumerConfig, ProducerConfig},
    consumer::KafkaConsumer,
    producer::KafkaProducer,
};
use s3::S3;
use state::{KafkaState, ServerData, ServerState};
use std::{sync::Arc, time::Duration};
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
        let router = Self::init_router().await;

        Self {
            tcp_listener,
            router,
        }
    }

    async fn init_tcp_listener() -> TcpListener {
        let host = read_env_var("HOST");
        let port = read_env_var("PORT");
        let addr = format!("{host}:{port}");

        TcpListener::bind(addr).await.expect("the address is busy")
    }

    async fn init_router() -> Router {
        let state = Self::init_state().await;

        Router::new()
            .route("/ping", routing::get(ping))
            .route("/images/upload/{user_id}", routing::post(upload_image))
            .route(
                "/images/{filename}",
                routing::get(download_image).delete(delete_image),
            )
            .with_state(state)
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

        let origins = read_env_var("ORIGINS")
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
        let producer_config = ProducerConfig::new(&brokers, &topic);
        let consumer_config = ConsumerConfig::new(brokers, group_id, topic, 0);
        let producer = KafkaProducer::new(producer_config).unwrap();
        let consumer = KafkaConsumer::new(consumer_config).unwrap();
        let kafka = KafkaState { producer, consumer };

        Arc::new(ServerData { s3, kafka })
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

fn read_env_var(key: &str) -> String {
    std::env::var(key).expect(&format!("{key} don`t set"))
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
