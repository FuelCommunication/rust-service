use kafka::{consumer::KafkaConsumer, producer::KafkaProducer};
use s3::S3;
use std::sync::Arc;

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub s3: S3,
    pub kafka: KafkaState,
}

pub struct KafkaState {
    pub producer: KafkaProducer,
    pub consumer: KafkaConsumer,
}
