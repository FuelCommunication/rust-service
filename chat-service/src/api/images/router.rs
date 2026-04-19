use super::schemas::Image;
use crate::{
    error::{ApiError, ApiResult, HttpError},
    state::ServerState,
};
use axum::extract::{Multipart, Path, State};
use kafka::schemas::{Action, KafkaMessage};
use s3::error::S3Error;
use uuid::Uuid;

#[tracing::instrument(skip(state))]
pub async fn upload_image(
    State(state): State<ServerState>,
    Path(user_id): Path<Uuid>,
    mut multipart: Multipart,
) -> ApiResult<Image> {
    if let Some(field) = multipart.next_field().await.unwrap() {
        let content_type = field
            .content_type()
            .map(ToString::to_string)
            .unwrap_or_else(|| "application/octet-stream".into());
        let data = field.bytes().await.unwrap();
        let key = Uuid::now_v7().to_string();

        state.s3.upload(&key, data, content_type).await?;

        let message = KafkaMessage {
            user_id: user_id.to_string(),
            action: Action::Create,
            data: Some(key.to_owned()),
        };
        state.kafka.producer.send(&message).await?;

        Ok(key.into())
    } else {
        Err(ApiError::Http(HttpError::InternalServerError(
            "Bucket is empty".to_owned(),
        )))
    }
}

#[tracing::instrument(skip(state))]
#[axum::debug_handler]
pub async fn download_image(
    State(state): State<ServerState>,
    Path(filename): Path<String>,
) -> ApiResult<Image> {
    let res = state.s3.download(&filename).await?;
    let body: Vec<u8> = res.body.collect().await.map_err(S3Error::from)?.to_vec();

    Ok((filename, body).into())
}

#[tracing::instrument(skip(state))]
pub async fn delete_image(
    State(state): State<ServerState>,
    Path(filename): Path<String>,
) -> ApiResult<Image> {
    state.s3.delete_object(&filename).await?;

    tracing::info!("Image deleted with filename: {filename}");

    Ok(filename.into())
}
