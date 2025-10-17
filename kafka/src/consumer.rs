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
            .set("enable.auto.commit", "true")
            .set("auto.commit.interval.ms", "5000")
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", "earliest")
            .set_log_level(config.log_level)
            .create::<StreamConsumer>()?;

        consumer.subscribe(&[&config.input_topic])?;

        Ok(Self {
            consumer,
            input_topic: config.input_topic,
        })
    }

    pub async fn consume(&self) -> KafkaResult<KafkaMessage> {
        tracing::debug!("Waiting for message from topic: {}", self.input_topic);
        let msg = self.consumer.recv().await?;
        tracing::info!("Received message from partition {}", msg.partition());
        let payload = msg.payload().ok_or_else(|| KafkaError::EmptyPayload {
            topic: self.input_topic.to_owned(),
        })?;

        let kafka_msg = serde_json::from_slice(payload).map_err(KafkaError::Serialization)?;
        self.consumer.store_offset_from_message(&msg)?;

        Ok(kafka_msg)
    }

    pub async fn close(self) -> KafkaResult<()> {
        self.consumer.unsubscribe();
        Ok(())
    }
}
