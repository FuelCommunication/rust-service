#![allow(dead_code)]

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use s3_client::error::S3Error;
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
    #[error("Unsupported media type")]
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Http(e) => e.into_response(),
            ApiError::S3(e) => s3_error_response(*e),
        }
    }
}

impl From<S3Error> for ApiError {
    fn from(err: S3Error) -> Self {
        ApiError::S3(Box::new(err))
    }
}

fn s3_error_response(err: S3Error) -> Response {
    let (status, error_type) = match &err {
        S3Error::GetObjectError(_) => (StatusCode::NOT_FOUND, "GetObjectError"),
        S3Error::HeaderObjectError(_) => (StatusCode::NOT_FOUND, "HeadObjectError"),
        S3Error::BucketNotEmpty => (StatusCode::CONFLICT, "BucketNotEmpty"),
        S3Error::MissingETag => (StatusCode::INTERNAL_SERVER_ERROR, "MissingETag"),
        S3Error::MissingUploadId => (StatusCode::INTERNAL_SERVER_ERROR, "MissingUploadId"),
        S3Error::IO(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IOError"),
        S3Error::ByteStreamError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ByteStreamError"),
        S3Error::BuildError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "BuildError"),
        S3Error::TokioJoin(_) => (StatusCode::INTERNAL_SERVER_ERROR, "TokioJoinError"),
        _ => (StatusCode::BAD_REQUEST, "S3Error"),
    };

    let body = Json(json!({
        "error": error_type,
        "message": err.to_string(),
    }));

    (status, body).into_response()
}
