use aws_sdk_s3::{
    error::{BuildError, SdkError},
    operation::{
        abort_multipart_upload::AbortMultipartUploadError,
        complete_multipart_upload::CompleteMultipartUploadError, copy_object::CopyObjectError,
        create_multipart_upload::CreateMultipartUploadError, delete_bucket::DeleteBucketError,
        delete_object::DeleteObjectError, delete_objects::DeleteObjectsError,
        get_object::GetObjectError, head_object::HeadObjectError,
        list_objects_v2::ListObjectsV2Error, put_object::PutObjectError,
        upload_part::UploadPartError,
    },
    primitives::ByteStreamError,
};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::io;

pub type S3Result<T> = Result<T, S3Error>;

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("Failed to get object from S3: {0}")]
    GetObjectError(#[from] SdkError<GetObjectError>),
    #[error("Failed to list objects in bucket: {0}")]
    ListObjectError(#[from] SdkError<ListObjectsV2Error>),
    #[error("Failed to put object to S3: {0}")]
    PutObjectError(#[from] SdkError<PutObjectError>),
    #[error("Failed to copy object: {0}")]
    CopyObjectError(#[from] SdkError<CopyObjectError>),
    #[error("Failed to upload part: {0}")]
    UploadPart(#[from] SdkError<UploadPartError>),
    #[error("Failed to create multipart upload: {0}")]
    CreateMultipart(#[from] SdkError<CreateMultipartUploadError>),
    #[error("Failed to complete multipart upload: {0}")]
    CompleteMultipart(#[from] SdkError<CompleteMultipartUploadError>),
    #[error("Failed to abort multipart upload: {0}")]
    AbortMultipart(#[from] SdkError<AbortMultipartUploadError>),
    #[error("Missing ETag in upload_part response")]
    MissingETag,
    #[error("Missing upload_id after CreateMultipartUpload")]
    MissingUploadId,
    #[error("Missing upload_id after CreateMultipartUpload")]
    HeaderObjectError(#[from] SdkError<HeadObjectError>),
    #[error("Failed to delete object: {0}")]
    DeleteObjectError(#[from] SdkError<DeleteObjectError>),
    #[error("Failed to delete multiple objects: {0}")]
    DeleteObjectsError(#[from] SdkError<DeleteObjectsError>),
    #[error("Failed to delete bucket: {0}")]
    DeleteBucketError(#[from] SdkError<DeleteBucketError>),
    #[error("Bucket is not empty — objects still remain inside")]
    BucketNotEmpty,
    #[error("I/O error: {0}")]
    IO(#[from] io::Error),
    #[error("ByteStream error: {0}")]
    ByteStreamError(#[from] ByteStreamError),
    #[error("SDK build error: {0}")]
    BuildError(#[from] BuildError),
    #[error("Tokio join error: {0}")]
    TokioJoin(String),
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let (status, e) = match self {
            Self::GetObjectError(e) => (StatusCode::BAD_REQUEST, format!("GetObject failed: {e}")),
            Self::ListObjectError(e) => {
                (StatusCode::BAD_REQUEST, format!("ListObjects failed: {e}"))
            }
            Self::PutObjectError(e) => (StatusCode::BAD_REQUEST, format!("PutObject failed: {e}")),
            Self::CopyObjectError(e) => {
                (StatusCode::BAD_REQUEST, format!("CopyObject failed: {e}"))
            }
            Self::UploadPart(e) => (StatusCode::BAD_REQUEST, format!("UploadPart failed: {e}")),
            Self::CreateMultipart(e) => (
                StatusCode::BAD_REQUEST,
                format!("CreateMultipart failed: {e}"),
            ),
            Self::CompleteMultipart(e) => (
                StatusCode::BAD_REQUEST,
                format!("CompleteMultipart failed: {e}"),
            ),
            Self::AbortMultipart(e) => (
                StatusCode::BAD_REQUEST,
                format!("AbortMultipart failed: {e}"),
            ),
            Self::HeaderObjectError(e) => {
                (StatusCode::BAD_REQUEST, format!("HeadObject failed: {e}"))
            }
            Self::DeleteObjectError(e) => {
                (StatusCode::BAD_REQUEST, format!("DeleteObject failed: {e}"))
            }
            Self::DeleteObjectsError(e) => (
                StatusCode::BAD_REQUEST,
                format!("DeleteObjects failed: {e}"),
            ),
            Self::DeleteBucketError(e) => {
                (StatusCode::BAD_REQUEST, format!("DeleteBucket failed: {e}"))
            }
            Self::BucketNotEmpty => (StatusCode::CONFLICT, "Bucket is not empty".to_string()),
            Self::MissingETag => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing ETag in response".to_string(),
            ),
            Self::MissingUploadId => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Missing upload_id".to_string(),
            ),
            Self::IO(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::ByteStreamError(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::BuildError(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            Self::TokioJoin(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        };

        let body = Json(json!({"error": e}));
        (status, body).into_response()
    }
}
