use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Forbidden: {0}")]
    Forbidden(String),
    #[error("Conflict: {0}")]
    Conflict(String),
    #[error("Internal server error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            Self::NotFound(e) => (StatusCode::NOT_FOUND, e),
            Self::Forbidden(e) => (StatusCode::FORBIDDEN, e),
            Self::Conflict(e) => (StatusCode::CONFLICT, e),
            Self::Internal(e) => {
                tracing::error!("Internal server error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error".to_owned())
            }
        };
        (status, Json(json!({"error": message}))).into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::NotFound("Resource not found".into()),
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => Self::Conflict("Resource already exists".into()),
            _ => Self::Internal(err.to_string()),
        }
    }
}
