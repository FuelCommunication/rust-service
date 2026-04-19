use crate::{
    config::ConsumerConfig,
    error::{KafkaError, KafkaResult},
};
use futures::Stream;
use rdkafka::{
    ClientConfig, Message,
    consumer::{Consumer, StreamConsumer},
};
use serde::de::DeserializeOwned;

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
            .set("session.timeout.ms", config.session_timeout_ms.to_string())
            .set("enable.auto.commit", config.auto_commit.to_string())
            .set("auto.commit.interval.ms", config.auto_commit_interval_ms.to_string())
            .set("enable.auto.offset.store", "false")
            .set("auto.offset.reset", config.auto_offset_reset.as_str())
            .set_log_level(config.log_level)
            .create::<StreamConsumer>()?;

        consumer.subscribe(&[&config.input_topic])?;

        tracing::info!(
            brokers = %config.brokers,
            group_id = %config.group_id,
            topic = %config.input_topic,
            "Kafka consumer started"
        );

        Ok(Self {
            consumer,
            input_topic: config.input_topic,
        })
    }

    pub async fn consume_raw(&self) -> KafkaResult<Vec<u8>> {
        tracing::debug!("Waiting for message from topic: {}", self.input_topic);
        let msg = self.consumer.recv().await?;
        tracing::info!("Received message from partition {}", msg.partition());

        let payload = msg
            .payload()
            .ok_or_else(|| KafkaError::EmptyPayload {
                topic: self.input_topic.to_owned(),
            })?
            .to_vec();

        self.consumer.store_offset_from_message(&msg)?;
        Ok(payload)
    }

    pub async fn consume<T: DeserializeOwned>(&self) -> KafkaResult<T> {
        let payload = self.consume_raw().await?;
        serde_json::from_slice(&payload).map_err(KafkaError::Serialization)
    }

    pub fn stream<T: DeserializeOwned + 'static>(&self) -> impl Stream<Item = KafkaResult<T>> + '_ {
        futures::stream::unfold(self, |consumer| async move {
            let result = consumer.consume::<T>().await;
            Some((result, consumer))
        })
    }

    pub async fn close(self) {
        self.consumer.unsubscribe();
        tracing::info!(topic = %self.input_topic, "Kafka consumer closed");
    }
}
