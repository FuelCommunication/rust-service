use rdkafka::error::RDKafkaErrorCode;

pub type KafkaResult<T> = Result<T, KafkaError>;
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
pub enum KafkaError {
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    #[error("Failed to encode message: {0}")]
    Encode(#[source] BoxError),
    #[error("Failed to decode message from {topic}[{partition}]@{offset}: {source}")]
    Decode {
        topic: String,
        partition: i32,
        offset: i64,
        #[source]
        source: BoxError,
    },
    #[error("Record from {topic}[{partition}]@{offset} has no payload (tombstone)")]
    EmptyPayload { topic: String, partition: i32, offset: i64 },
    #[error("No topic given and no default topic configured")]
    NoTopic,
    #[error("Delivery result channel was closed")]
    DeliveryCanceled(#[from] futures_channel::oneshot::Canceled),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Background task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

impl KafkaError {
    pub fn code(&self) -> Option<RDKafkaErrorCode> {
        match self {
            Self::Kafka(e) => e.rdkafka_error_code(),
            _ => None,
        }
    }

    pub fn is_poison(&self) -> bool {
        matches!(self, Self::Decode { .. } | Self::EmptyPayload { .. })
    }
}
