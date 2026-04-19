use crate::{
    config::ConsumerConfig,
    error::{KafkaError, KafkaResult},
    schemas::KafkaMessage,
};
use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};

pub struct KafkaConsumer {
    consumer: StreamConsumer,
    pub input_topic: String,
}

impl KafkaConsumer {
    pub fn new(config: ConsumerConfig) -> KafkaResult<Self> {
        let consumer = ClientConfig::new()
            .set("group.id", &config.group_id)
            .set("bootstrap.servers", &config.brokers)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "6000")
            .set("enable.auto.offset.store", "false")
            .set_log_level(config.log_level)
            .create::<StreamConsumer>()?;

        consumer.subscribe(&[&config.input_topic])?;

        Ok(Self {
            consumer,
            input_topic: config.input_topic,
        })
    }

    pub async fn consume(&self) -> KafkaResult<KafkaMessage> {
        let msg = self.consumer.recv().await?;
        let payload = msg
            .payload()
            .ok_or_else(|| KafkaError::Kafka(rdkafka::error::KafkaError::NoMessageReceived))?;
        let kafka_msg: KafkaMessage = serde_json::from_slice(payload)?;
        Ok(kafka_msg)
    }
}
