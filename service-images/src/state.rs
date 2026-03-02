use kafka_client::{
    config::{ConsumerConfig, LogLevel, ProducerConfig},
    consumer::KafkaConsumer,
    producer::KafkaProducer,
};
use s3_client::S3;
use std::sync::Arc;

use crate::Config;

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub s3: S3,
    pub broker: KafkaState,
}

pub struct KafkaState {
    pub producer: KafkaProducer,
    pub consumer: KafkaConsumer,
}

impl ServerData {
    pub async fn new(config: &Config) -> ServerState {
        let bucket: &'static str = Box::leak(config.s3.bucket.clone().into_boxed_str());
        let s3 = S3::new(
            config.s3.access_key.clone(),
            config.s3.secret_key.clone(),
            config.s3.region.clone(),
            config.s3.endpoint_url.clone(),
            bucket,
        )
        .await;

        let producer_config = ProducerConfig::builder(&config.kafka.brokers, &config.kafka.topic)
            .auto_create_topics(true)
            .build()
            .expect("Invalid producer config");
        let consumer_config = ConsumerConfig::builder(
            config.kafka.brokers.clone(),
            config.kafka.group_id.clone(),
            config.kafka.topic.clone(),
        )
        .log_level(LogLevel::Debug)
        .build()
        .expect("Invalid consumer config");

        let producer = KafkaProducer::new(producer_config).unwrap();
        let consumer = KafkaConsumer::new(consumer_config).unwrap();
        let broker = KafkaState { producer, consumer };

        Arc::new(ServerData { s3, broker })
    }
}
