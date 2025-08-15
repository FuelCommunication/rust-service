use super::schemas::Image;
use crate::{
    api_response::{ApiError, ApiResult},
    state::ServerState,
};
use axum::extract::{Multipart, Path, State};

#[tracing::instrument(skip(state))]
pub async fn upload_image(
    State(state): State<ServerState>,
    mut multipart: Multipart,
) -> ApiResult<Image> {
    if let Some(field) = multipart.next_field().await.unwrap() {
        let content_type = field
            .content_type()
            .map(ToString::to_string)
            .unwrap_or_else(|| "application/octet-stream".into());
        let data = field.bytes().await.unwrap();
        let key = uuid::Uuid::now_v7().to_string();

        state
            .s3
            .upload(&key, data, content_type)
            .await
            .map_err(|err| ApiError::InternalServerError(err.to_string()))?;
        Ok(key.into())
    } else {
        Err(ApiError::InternalServerError("Bucket is empty".to_owned()))
    }
}

#[tracing::instrument(skip(state))]
pub async fn download_image(
    State(state): State<ServerState>,
    Path(filename): Path<String>,
) -> Result<Image, ApiError> {
    let res = state
        .s3
        .download(&filename)
        .await
        .map_err(|err| ApiError::InternalServerError(err.to_string()))?;
    let body: Vec<u8> = res
        .body
        .collect()
        .await
        .map_err(|err| ApiError::InternalServerError(err.to_string()))?
        .to_vec();

    Ok((filename, body).into())
}

#[tracing::instrument(skip(state))]
pub async fn delete_image(
    State(state): State<ServerState>,
    Path(filename): Path<String>,
) -> Result<Image, ApiError> {
    state
        .s3
        .delete_object(&filename)
        .await
        .map_err(|err| ApiError::InternalServerError(err.to_string()))?;

    tracing::info!("Image deleted with filename: {filename}");

    Ok(filename.into())
}
