use crate::api_response::ApiError;
use axum::Json;
use serde_json::json;

pub async fn ping() -> Json<serde_json::Value> {
    Json(json!({"ping": "pong!"}))
}

pub async fn not_found() -> ApiError {
    ApiError::NotFound
}

