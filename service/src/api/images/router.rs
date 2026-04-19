use super::schemas::Image;
use crate::{
    error::{ApiError, ApiResult, HttpError},
    state::ServerState,
};
use axum::extract::{Multipart, Path, State};
use kafka::schemas::{Action, KafkaMessage};
use uuid::Uuid;

const MAX_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB
const ALLOWED_CONTENT_TYPES: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];

#[tracing::instrument(skip(state, multipart))]
pub async fn upload_image(
    State(state): State<ServerState>,
    Path(user_id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Image> {
    let field = multipart
        .next_field()
        .await
        .map_err(|e| {
            tracing::error!("Failed to read multipart field: {:?}", e);
            HttpError::BadRequest("Invalid multipart data".into())
        })?
        .ok_or_else(|| {
            tracing::warn!("No file provided in upload request");
            HttpError::NotFound("File not found".into())
        })?;

    let content_type = field
        .content_type()
        .map(ToString::to_string)
        .unwrap_or_else(|| "application/octet-stream".into());
    validate_content_type(&content_type)?;

    let data = field.bytes().await.map_err(|e| {
        tracing::error!("Failed to read file bytes: {:?}", e);
        HttpError::BadRequest("Failed to read uploaded file".into())
    })?;
    if data.len() > MAX_FILE_SIZE {
        tracing::warn!("File too large: {} bytes", data.len());
        return Err(ApiError::Http(HttpError::BadRequest("File too large".into())));
    }

    let key = Uuid::now_v7().to_string();
    let message = KafkaMessage {
        user_id: user_id.to_string(),
        action: Action::Create,
        data: Some(key.clone()),
    };

    let (s3_res, kafka_res) = tokio::join!(
        state.s3.upload(&key, data, &content_type),
        state.kafka.producer.send(&message)
    );

    if let Err(e) = s3_res {
        tracing::error!("Error uploading to S3: {:?}", e);
        return Err(ApiError::Http(HttpError::Internal("Failed to upload file".into())));
    }
    if let Err(err) = kafka_res {
        tracing::error!("Error sending Kafka message: {:?}", err);
    }

    Ok(key.into())
}

pub async fn download_image(State(state): State<ServerState>, Path(filename): Path<String>) -> ApiResult<Image> {
    validate_filename(&filename)?;
    let body = state.s3.download(&filename).await?;
    Ok((filename, body).into())
}

pub async fn delete_image(State(state): State<ServerState>, Path(filename): Path<String>) -> ApiResult<Image> {
    validate_filename(&filename)?;
    let exists = state.s3.object_exists(&filename).await?;

    if !exists {
        tracing::warn!("File not found: {}", filename);
        return Err(ApiError::Http(HttpError::NotFound(format!("Image {} not found", filename))));
    }

    state.s3.delete_object(&filename).await?;

    Ok(filename.into())
}

fn validate_filename(filename: &str) -> Result<(), HttpError> {
    if filename.is_empty() || !filename.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        tracing::warn!("Invalid filename: {}", filename);
        return Err(HttpError::BadRequest("Invalid filename".to_owned()));
    }
    Ok(())
}

fn validate_content_type(content_type: &str) -> Result<(), HttpError> {
    if !ALLOWED_CONTENT_TYPES.contains(&content_type) {
        tracing::warn!("Invalid content type: {}", content_type);
        Err(HttpError::UnsupportedMediaType)
    } else {
        Ok(())
    }
}
