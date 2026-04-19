use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use rdkafka::{error::RDKafkaError, message::OwnedMessage, producer::FutureRecord};
use serde_json::json;

pub type KafkaResult<T> = Result<T, KafkaError>;

#[derive(Debug, thiserror::Error)]
pub enum KafkaError {
    #[error("RDKafka error: {0}")]
    RDKafka(#[from] RDKafkaError),
    #[error("Kafka error: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    #[error("De/serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("Oneshot message was canceled")]
    CanceledMessage(#[from] futures::channel::oneshot::Canceled),
}

impl
    From<(
        rdkafka::error::KafkaError,
        FutureRecord<'_, String, Vec<u8>>,
    )> for KafkaError
{
    fn from(e: (rdkafka::error::KafkaError, FutureRecord<String, Vec<u8>>)) -> Self {
        Self::Kafka(e.0)
    }
}

impl From<(rdkafka::error::KafkaError, OwnedMessage)> for KafkaError {
    fn from(e: (rdkafka::error::KafkaError, OwnedMessage)) -> Self {
        Self::Kafka(e.0)
    }
}

impl IntoResponse for KafkaError {
    fn into_response(self) -> Response {
        let (status, e) = match self {
            Self::Kafka(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::RDKafka(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::SerdeJson(e) => (StatusCode::BAD_REQUEST, e.to_string()),
            Self::CanceledMessage(e) => (StatusCode::BAD_REQUEST, e.to_string()),
        };

        let body = Json(json!({"error": e}));
        (status, body).into_response()
    }
}
