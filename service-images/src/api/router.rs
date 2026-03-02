use super::schemas::Image;
use crate::{
    error::{ApiError, ApiResult, HttpError},
    state::ServerState,
};
use axum::extract::{Multipart, Path, State};
use kafka_client::schemas::{Action, KafkaMessage};
use uuid::Uuid;

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

    let key = Uuid::now_v7().to_string();
    let message = KafkaMessage {
        user_id: user_id.to_string(),
        action: Action::Create,
        data: Some(key.clone()),
    };

    let (s3_res, kafka_res) = tokio::join!(
        state.s3.upload(&key, data, &content_type),
        state.broker.producer.send(&message.user_id, &message)
    );

    if let Err(e) = s3_res {
        tracing::error!("Error uploading to S3: {:?}", e);
        return Err(ApiError::Http(HttpError::Internal("Failed to upload file".into())));
    }
    if let Err(err) = kafka_res {
        tracing::error!("Error sending Kafka message: {:?}", err);
    }

    Ok(Image::Created(key))
}

pub async fn download_image(State(state): State<ServerState>, Path(filename): Path<String>) -> ApiResult<Image> {
    validate_filename(&filename)?;
    let object = state.s3.download(&filename).await?;
    Ok(Image::File {
        filename,
        data: object.data,
        content_type: object.content_type.unwrap_or_else(|| "application/octet-stream".into()),
    })
}

#[tracing::instrument(skip(state))]
pub async fn delete_image(State(state): State<ServerState>, Path(filename): Path<String>) -> ApiResult<Image> {
    validate_filename(&filename)?;

    let exists = state.s3.object_exists(&filename).await?;
    if !exists {
        tracing::warn!("File not found: {}", filename);
        return Err(ApiError::Http(HttpError::NotFound(format!("Image {} not found", filename))));
    }

    state.s3.delete_object(&filename).await?;

    let message = KafkaMessage {
        user_id: String::new(),
        action: Action::Delete,
        data: Some(filename.clone()),
    };
    if let Err(err) = state.broker.producer.send(&filename, &message).await {
        tracing::error!("Error sending Kafka delete event: {:?}", err);
    }

    Ok(Image::Deleted(filename))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_filename_valid_alphanumeric() {
        assert!(validate_filename("abc123").is_ok());
    }

    #[test]
    fn validate_filename_valid_with_dashes_and_underscores() {
        assert!(validate_filename("my-file_name-123").is_ok());
    }

    #[test]
    fn validate_filename_valid_uuid() {
        assert!(validate_filename("01961f3a-7c44-7e38-bdd5-abc123def456").is_ok());
    }

    #[test]
    fn validate_filename_empty() {
        assert!(validate_filename("").is_err());
    }

    #[test]
    fn validate_filename_with_dots() {
        assert!(validate_filename("file.txt").is_err());
    }

    #[test]
    fn validate_filename_with_slashes() {
        assert!(validate_filename("../etc/passwd").is_err());
    }

    #[test]
    fn validate_filename_with_spaces() {
        assert!(validate_filename("file name").is_err());
    }

    #[test]
    fn validate_filename_with_special_chars() {
        assert!(validate_filename("file@name!").is_err());
    }

    #[test]
    fn validate_content_type_jpeg() {
        assert!(validate_content_type("image/jpeg").is_ok());
    }

    #[test]
    fn validate_content_type_png() {
        assert!(validate_content_type("image/png").is_ok());
    }

    #[test]
    fn validate_content_type_gif() {
        assert!(validate_content_type("image/gif").is_ok());
    }

    #[test]
    fn validate_content_type_webp() {
        assert!(validate_content_type("image/webp").is_ok());
    }

    #[test]
    fn validate_content_type_text_rejected() {
        assert!(validate_content_type("text/plain").is_err());
    }

    #[test]
    fn validate_content_type_empty_rejected() {
        assert!(validate_content_type("").is_err());
    }

    #[test]
    fn validate_content_type_octet_stream_rejected() {
        assert!(validate_content_type("application/octet-stream").is_err());
    }
}
