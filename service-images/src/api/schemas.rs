use axum::{
    Json,
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub enum Image {
    Created(String),
    Deleted(String),
    File {
        filename: String,
        data: Vec<u8>,
        content_type: String,
    },
}

impl IntoResponse for Image {
    fn into_response(self) -> Response {
        match self {
            Self::Created(name) => (StatusCode::CREATED, Json(json!({"filename": name}))).into_response(),
            Self::Deleted(name) => (StatusCode::OK, Json(json!({"filename": name}))).into_response(),
            Self::File {
                filename,
                data,
                content_type,
            } => Response::builder()
                .header("Content-Disposition", format!("attachment; filename=\"{filename}\""))
                .header("Content-Type", content_type)
                .body(Body::from(data))
                .unwrap(),
        }
    }
}
