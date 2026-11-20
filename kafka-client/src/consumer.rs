use std::{future::Future, sync::Arc, time::Duration};

use futures_util::Stream;
use rdkafka::consumer::{CommitMode, Consumer as _, StreamConsumer};

use crate::{
    codec::{Decode, JsonCodec},
    config::ConsumerConfig,
    context::TracingContext,
    error::{KafkaError, KafkaResult},
    record::{Message, Record},
};

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(10),
        }
    }
}

impl RetryPolicy {
    fn backoff(&self, attempt: u32) -> Duration {
        self.initial_backoff
            .saturating_mul(1u32 << attempt.min(16))
            .min(self.max_backoff)
    }
}

pub struct KafkaConsumer<C = JsonCodec> {
    consumer: Arc<StreamConsumer<TracingContext>>,
    group_id: String,
    commits_enabled: bool,
    codec: C,
}

impl KafkaConsumer<JsonCodec> {
    pub fn new(config: ConsumerConfig) -> KafkaResult<Self> {
        Self::with_codec(config, JsonCodec)
    }
}

impl<C> KafkaConsumer<C> {
    pub fn with_codec(config: ConsumerConfig, codec: C) -> KafkaResult<Self> {
        config.validate()?;
        let group_id = config.group_id();
        let commits_enabled = config.commits_enabled();
        let context = TracingContext {
            on_statistics: config.connection.on_statistics.clone(),
            commit_on_revoke: commits_enabled,
        };
        let consumer: StreamConsumer<TracingContext> = config.to_client_config(&group_id).create_with_context(context)?;

        let topics: Vec<&str> = config.topics.iter().map(String::as_str).collect();
        consumer.subscribe(&topics)?;

        tracing::info!(
            brokers = %config.connection.brokers,
            group_id = %group_id,
            topics = ?config.topics,
            mode = ?config.mode,
            "Kafka consumer subscribed"
        );

        Ok(Self {
            consumer: Arc::new(consumer),
            group_id,
            commits_enabled,
            codec,
        })
    }

    pub fn group_id(&self) -> &str {
        &self.group_id
    }

    pub async fn recv(&self) -> KafkaResult<Record> {
        let msg = self.consumer.recv().await?;
        let record = Record::from_borrowed(&msg);
        tracing::trace!(topic = %record.topic, partition = record.partition, offset = record.offset, "Received record");
        Ok(record)
    }

    pub async fn recv_decoded<T>(&self) -> KafkaResult<Message<T>>
    where
        C: Decode<T>,
    {
        let record = self.recv().await?;
        self.decode(record)
    }

    pub fn decode<T>(&self, record: Record) -> KafkaResult<Message<T>>
    where
        C: Decode<T>,
    {
        let Some(payload) = record.payload.as_deref() else {
            return Err(KafkaError::EmptyPayload {
                topic: record.topic,
                partition: record.partition,
                offset: record.offset,
            });
        };
        match self.codec.decode(payload) {
            Ok(value) => Ok(Message { value, record }),
            Err(source) => Err(KafkaError::Decode {
                topic: record.topic,
                partition: record.partition,
                offset: record.offset,
                source,
            }),
        }
    }

    pub fn ack(&self, record: &Record) -> KafkaResult<()> {
        self.ack_offset(&record.topic, record.partition, record.offset)
    }

    pub fn ack_offset(&self, topic: &str, partition: i32, offset: i64) -> KafkaResult<()> {
        self.consumer.store_offset(topic, partition, offset)?;
        Ok(())
    }

    pub async fn commit(&self) -> KafkaResult<()> {
        if !self.commits_enabled {
            return Ok(());
        }
        let consumer = self.consumer.clone();
        match tokio::task::spawn_blocking(move || consumer.commit_consumer_state(CommitMode::Sync)).await? {
            Ok(()) => Ok(()),
            Err(e) if matches!(e.rdkafka_error_code(), Some(rdkafka::types::RDKafkaErrorCode::NoOffset)) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn stream(&self) -> impl Stream<Item = KafkaResult<Record>> + '_ {
        futures_util::stream::unfold(self, |consumer| async move { Some((consumer.recv().await, consumer)) })
    }

    pub async fn run<T, F, Fut, E>(&self, retry: RetryPolicy, shutdown: impl Future<Output = ()>, mut handler: F)
    where
        C: Decode<T>,
        F: FnMut(Message<T>) -> Fut,
        Fut: Future<Output = Result<(), E>>,
        E: std::fmt::Display,
        T: Clone,
    {
        let mut shutdown = std::pin::pin!(shutdown);
        loop {
            let record = tokio::select! {
                biased;
                () = &mut shutdown => break,
                record = self.recv() => record,
            };
            let record = match record {
                Ok(record) => record,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to receive Kafka record");
                    tokio::time::sleep(retry.initial_backoff).await;
                    continue;
                }
            };

            let (topic, partition, offset) = (record.topic.clone(), record.partition, record.offset);
            match self.decode::<T>(record) {
                Ok(message) => self.handle_with_retry(&retry, &mut handler, message).await,
                Err(e) => tracing::warn!(error = %e, "Skipping undecodable Kafka record"),
            }
            if let Err(e) = self.ack_offset(&topic, partition, offset) {
                tracing::error!(error = %e, topic, partition, offset, "Failed to store offset");
            }
        }
        self.close().await;
    }

    async fn handle_with_retry<T, F, Fut, E>(&self, retry: &RetryPolicy, handler: &mut F, message: Message<T>)
    where
        F: FnMut(Message<T>) -> Fut,
        Fut: Future<Output = Result<(), E>>,
        E: std::fmt::Display,
        T: Clone,
    {
        let mut attempt = 0;
        loop {
            match handler(message.clone()).await {
                Ok(()) => return,
                Err(e) if attempt < retry.max_retries => {
                    tracing::warn!(error = %e, attempt, topic = %message.record.topic, offset = message.record.offset, "Handler failed, retrying");
                    tokio::time::sleep(retry.backoff(attempt)).await;
                    attempt += 1;
                }
                Err(e) => {
                    tracing::error!(error = %e, topic = %message.record.topic, partition = message.record.partition, offset = message.record.offset, "Handler failed, skipping record");
                    return;
                }
            }
        }
    }

    pub async fn close(&self) {
        if let Err(e) = self.commit().await {
            tracing::warn!(error = %e, "Failed to commit offsets on close");
        }
        self.consumer.unsubscribe();
        tracing::info!(group_id = %self.group_id, "Kafka consumer closed");
    }
}
