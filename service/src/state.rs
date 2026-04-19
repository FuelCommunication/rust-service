use kafka::{consumer::KafkaConsumer, producer::KafkaProducer};
use s3::S3;
use scylladb::ChatMessageStore;
use std::sync::Arc;

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub s3: S3,
    pub broker: KafkaState,
    pub message_store: ChatMessageStore,
}

pub struct KafkaState {
    pub producer: KafkaProducer,
    pub consumer: KafkaConsumer,
}
