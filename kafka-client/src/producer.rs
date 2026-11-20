use std::time::Duration;

use rdkafka::producer::{FutureProducer, FutureRecord, Producer as _};

use crate::{
    codec::{Encode, JsonCodec},
    config::ProducerConfig,
    context::TracingContext,
    error::{KafkaError, KafkaResult},
    record::{DeliveryReport, Headers},
};

#[derive(Debug, Clone, Default)]
pub struct SendOptions<'a> {
    pub topic: Option<&'a str>,
    pub key: Option<&'a [u8]>,
    pub headers: Headers,
    pub partition: Option<i32>,
    pub timestamp: Option<i64>,
}

impl<'a> SendOptions<'a> {
    pub fn key(key: &'a str) -> Self {
        Self {
            key: Some(key.as_bytes()),
            ..Default::default()
        }
    }

    pub fn topic(mut self, topic: &'a str) -> Self {
        self.topic = Some(topic);
        self
    }

    pub fn header(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.headers.push(key, value);
        self
    }
}

#[derive(Clone)]
pub struct KafkaProducer<C = JsonCodec> {
    producer: FutureProducer<TracingContext>,
    default_topic: Option<String>,
    queue_timeout: Duration,
    codec: C,
}

impl KafkaProducer<JsonCodec> {
    pub fn new(config: ProducerConfig) -> KafkaResult<Self> {
        Self::with_codec(config, JsonCodec)
    }
}

impl<C> KafkaProducer<C> {
    pub fn with_codec(config: ProducerConfig, codec: C) -> KafkaResult<Self> {
        config.validate()?;
        let context = TracingContext {
            on_statistics: config.connection.on_statistics.clone(),
            commit_on_revoke: false,
        };
        let producer = config.to_client_config().create_with_context(context)?;

        tracing::info!(
            brokers = %config.connection.brokers,
            default_topic = ?config.default_topic,
            "Kafka producer created"
        );

        Ok(Self {
            producer,
            default_topic: config.default_topic,
            queue_timeout: config.queue_timeout,
            codec,
        })
    }

    pub async fn send<T>(&self, key: &str, value: &T) -> KafkaResult<DeliveryReport>
    where
        C: Encode<T>,
        T: ?Sized,
    {
        self.send_with(SendOptions::key(key), value).await
    }

    pub async fn send_with<T>(&self, options: SendOptions<'_>, value: &T) -> KafkaResult<DeliveryReport>
    where
        C: Encode<T>,
        T: ?Sized,
    {
        let topic = self.resolve_topic(options.topic)?;
        let payload = self.codec.encode(value).map_err(KafkaError::Encode)?;
        let headers = self.headers_with_content_type::<T>(options.headers);
        let record = build_record(topic, &payload, options.key, &headers, options.partition, options.timestamp);

        let delivery = self
            .producer
            .send(record, self.queue_timeout)
            .await
            .map_err(|(err, _)| KafkaError::Kafka(err))?;

        tracing::debug!(
            topic,
            partition = delivery.partition,
            offset = delivery.offset,
            "Message delivered"
        );
        Ok(DeliveryReport {
            partition: delivery.partition,
            offset: delivery.offset,
        })
    }

    pub fn send_detached<T>(&self, options: SendOptions<'_>, value: &T) -> KafkaResult<()>
    where
        C: Encode<T>,
        T: ?Sized,
    {
        let topic = self.resolve_topic(options.topic)?;
        let payload = self.codec.encode(value).map_err(KafkaError::Encode)?;
        let headers = self.headers_with_content_type::<T>(options.headers);
        let record = build_record(topic, &payload, options.key, &headers, options.partition, options.timestamp);

        let delivery = self.producer.send_result(record).map_err(|(err, _)| KafkaError::Kafka(err))?;
        let topic = topic.to_owned();
        tokio::spawn(async move {
            match delivery.await {
                Ok(Ok(_)) => {}
                Ok(Err((err, _))) => tracing::error!(topic, error = %err, "Detached message delivery failed"),
                Err(_) => tracing::error!(topic, "Detached message delivery result lost"),
            }
        });
        Ok(())
    }

    pub fn in_flight(&self) -> i32 {
        self.producer.in_flight_count()
    }

    pub async fn flush(&self, timeout: Duration) -> KafkaResult<()>
    where
        C: Clone + Send + 'static,
    {
        let producer = self.producer.clone();
        tokio::task::spawn_blocking(move || producer.flush(timeout)).await??;
        Ok(())
    }

    pub async fn shutdown(self, timeout: Duration) -> KafkaResult<()>
    where
        C: Clone + Send + 'static,
    {
        let result = self.flush(timeout).await;
        match &result {
            Ok(()) => tracing::info!("Kafka producer shut down"),
            Err(e) => {
                tracing::warn!(error = %e, in_flight = self.in_flight(), "Kafka producer shut down with undelivered messages")
            }
        }
        result
    }

    fn resolve_topic<'a>(&'a self, topic: Option<&'a str>) -> KafkaResult<&'a str> {
        topic.or(self.default_topic.as_deref()).ok_or(KafkaError::NoTopic)
    }

    fn headers_with_content_type<T: ?Sized>(&self, mut headers: Headers) -> Headers
    where
        C: Encode<T>,
    {
        if !headers.contains("content-type") {
            headers.push("content-type", self.codec.content_type());
        }
        headers
    }
}

fn build_record<'a>(
    topic: &'a str,
    payload: &'a [u8],
    key: Option<&'a [u8]>,
    headers: &Headers,
    partition: Option<i32>,
    timestamp: Option<i64>,
) -> FutureRecord<'a, [u8], [u8]> {
    let mut record = FutureRecord::to(topic).payload(payload).headers(headers.to_owned_headers());
    if let Some(key) = key {
        record = record.key(key);
    }
    if let Some(partition) = partition {
        record = record.partition(partition);
    }
    if let Some(timestamp) = timestamp {
        record = record.timestamp(timestamp);
    }
    record
}
