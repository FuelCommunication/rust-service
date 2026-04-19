use crate::{
    config::ProducerConfig,
    error::{KafkaError, KafkaResult},
};
use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord, Producer},
};
use serde::Serialize;
use std::time::Duration;

pub struct KafkaProducer {
    producer: FutureProducer,
    topic: String,
}

impl KafkaProducer {
    pub fn new(config: ProducerConfig) -> KafkaResult<Self> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", &config.brokers)
            .set("message.timeout.ms", config.message_timeout_ms.to_string())
            .set("allow.auto.create.topics", config.auto_create_topics.to_string())
            .set("retries", config.retries.to_string())
            .create::<FutureProducer>()?;

        tracing::info!(
            brokers = %config.brokers,
            topic = %config.topic,
            retries = config.retries,
            "Kafka producer started"
        );

        Ok(Self {
            producer,
            topic: config.topic,
        })
    }

    pub async fn send<T: Serialize>(&self, key: &str, payload: &T) -> KafkaResult<()> {
        let bytes = serde_json::to_vec(payload)?;
        tracing::debug!(topic = %self.topic, key = %key, "Sending message");

        let record = FutureRecord::to(&self.topic).payload(&bytes).key(key);
        let delivery_future = self.producer.send_result(record).map_err(|(err, _)| KafkaError::Kafka(err))?;

        delivery_future
            .await
            .map_err(KafkaError::CanceledMessage)?
            .map_err(|(err, _)| KafkaError::Kafka(err))?;

        tracing::info!(topic = %self.topic, key = %key, "Message sent successfully");
        Ok(())
    }

    pub fn flush(&self, timeout: Duration) -> KafkaResult<()> {
        self.producer.flush(timeout)?;
        tracing::debug!(topic = %self.topic, "Flush producer");
        Ok(())
    }
}

impl Drop for KafkaProducer {
    fn drop(&mut self) {
        let _ = self.producer.flush(Duration::from_secs(5));
        tracing::info!(topic = %self.topic, "Kafka producer closed");
    }
}
