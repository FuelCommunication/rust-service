use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Internal server error: {0}")]
    Internal(String),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, e) = match self {
            Self::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            Self::NotFound(e) => (StatusCode::NOT_FOUND, e),
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
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::Http(e) => e.into_response(),
        }
    }
}
