pub mod images;

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde_json::json;

pub async fn ping() -> Json<serde_json::Value> {
    Json(json!({"ping": "pong!"}))
}

pub async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"})))
}
