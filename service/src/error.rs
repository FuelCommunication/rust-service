#![allow(dead_code)]

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kafka::error::KafkaError;
use s3::error::S3Error;
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal server error: {0}")]
    Internal(String),
    #[error("Not implemented")]
    NotImplemented,
    #[error("Not implemented")]
    UnsupportedMediaType,
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, e) = match self {
            Self::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            Self::Forbidden(e) => (StatusCode::FORBIDDEN, e),
            Self::NotFound(e) => (StatusCode::NOT_FOUND, e),
            Self::NotImplemented => (StatusCode::NOT_IMPLEMENTED, "Not implemented".to_owned()),
            Self::UnsupportedMediaType => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Unsupported media type".to_owned()),
            Self::Internal(e) => {
                tracing::error!("Internal server error: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error".to_owned())
            }
        };

        let body = Json(json!({"error": e}));
        (status, body).into_response()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Http error: {0}")]
    Http(#[from] HttpError),
    #[error("S3 error: {0}")]
    S3(Box<S3Error>),
    #[error("Kafka error: {0}")]
    Kafka(#[from] KafkaError),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Http(e) => e.into_response(),
            ApiError::S3(e) => e.into_response(),
            ApiError::Kafka(e) => e.into_response(),
        }
    }
}

impl From<S3Error> for ApiError {
    fn from(err: S3Error) -> Self {
        ApiError::S3(Box::new(err))
    }
}
