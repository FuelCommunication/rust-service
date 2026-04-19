use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub enum ApiError {
    BadRequest,
    Forbidden,
    NotFound,
    RequestTimeout,
    InternalServerError(String),
    NotImplemented,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, err) = match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST, "Bad request".to_owned()),
            Self::Forbidden => (StatusCode::FORBIDDEN, "Forbidden".to_owned()),
            Self::NotFound => (StatusCode::NOT_FOUND, "Recurse not found".to_owned()),
            Self::RequestTimeout => (StatusCode::REQUEST_TIMEOUT, "Request timeout".to_owned()),
            Self::NotImplemented => (StatusCode::NOT_IMPLEMENTED, "Not implemented".to_owned()),
            Self::InternalServerError(err) => {
                tracing::error!("Internal server error: {}", err);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal Server Error".to_owned(),
                )
            }
        };

        let body = Json(json!({"error": err}));
        (status, body.to_owned()).into_response()
    }
}
