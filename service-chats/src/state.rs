use crate::{Config, api::schemas::ServerEvent};
use dashmap::DashMap;
use scylladb_client::{ChatMessageStore, ScyllaConfig};
use std::sync::Arc;
use tokio::sync::broadcast;

pub type ServerState = Arc<ServerData>;

pub struct Room {
    pub sender: broadcast::Sender<ServerEvent>,
}

pub struct ServerData {
    pub message_store: ChatMessageStore,
    pub rooms: DashMap<String, Room>,
    pub broadcast_buffer_size: usize,
    pub http_client: reqwest::Client,
    pub channels_service_url: String,
}

impl ServerData {
    pub async fn new(config: &Config) -> ServerState {
        let scylla_config = ScyllaConfig {
            uri: config.scylla_url.clone(),
            additional_nodes: config
                .scylla_nodes
                .split(',')
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
            replication_factor: config.scylla_replication_factor,
            ..Default::default()
        };
        let message_store = ChatMessageStore::new(&scylla_config, true).await.unwrap();

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("Failed to build HTTP client");

        Arc::new(ServerData {
            message_store,
            rooms: DashMap::with_capacity(10_000),
            broadcast_buffer_size: config.broadcast_buffer_size,
            http_client,
            channels_service_url: config.channels_service_url.clone(),
        })
    }
}
