pub mod router;
pub(crate) mod schemas;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::json;

use crate::state::ServerState;

pub async fn ping() -> Json<serde_json::Value> {
    Json(json!({"ping": "pong!"}))
}

pub async fn health(State(state): State<ServerState>) -> impl IntoResponse {
    if state.message_store.health_check().await {
        (StatusCode::OK, Json(json!({"status": "ok"})))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"status": "unavailable"})))
    }
}

pub async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "Not found"})))
}
