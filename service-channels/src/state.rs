use std::sync::Arc;

use kafka_client::producer::KafkaProducer;
use valkey_client::Valkey;

use crate::{search::SearchService, store::ChannelStore};

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub store: ChannelStore,
    pub cache: Valkey,
    pub cache_ttl: u64,
    pub search: SearchService,
    pub producer: KafkaProducer,
}
