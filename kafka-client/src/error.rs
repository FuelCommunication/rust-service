use rdkafka::error::RDKafkaError;

pub type KafkaResult<T> = Result<T, KafkaError>;

#[derive(Debug, thiserror::Error)]
pub enum KafkaError {
    #[error("RDKafka initialization error: {0}")]
    RDKafka(#[from] RDKafkaError),
    #[error("Kafka operation error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    #[error("Message serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Message was canceled or channel closed")]
    CanceledMessage(#[from] futures::channel::oneshot::Canceled),
    #[error("Empty message payload received from topic: {topic}")]
    EmptyPayload { topic: String },
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}
