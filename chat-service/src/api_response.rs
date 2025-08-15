#![allow(dead_code)]

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type ApiResult<T, E = ApiError> = Result<T, E>;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Forbidden(String),
    NotFound(String),
    InternalServerError(String),
    NotImplemented,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, e) = match self {
            Self::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            Self::Forbidden(e) => (StatusCode::FORBIDDEN, e),
            Self::NotFound(e) => (StatusCode::NOT_FOUND, e),
            Self::NotImplemented => (StatusCode::NOT_IMPLEMENTED, "Not implemented".to_owned()),
            Self::InternalServerError(e) => {
                tracing::error!("Internal server error: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error".to_owned(),
                )
            }
        };

        let body = Json(json!({"error": e}));
        (status, body.to_owned()).into_response()
    }
}
