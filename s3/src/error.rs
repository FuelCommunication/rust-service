use aws_sdk_s3::{
    error::{BuildError, SdkError},
    operation::{
        abort_multipart_upload::AbortMultipartUploadError, complete_multipart_upload::CompleteMultipartUploadError,
        copy_object::CopyObjectError, create_multipart_upload::CreateMultipartUploadError, delete_bucket::DeleteBucketError,
        delete_object::DeleteObjectError, delete_objects::DeleteObjectsError, get_object::GetObjectError,
        head_object::HeadObjectError, list_objects_v2::ListObjectsV2Error, put_object::PutObjectError,
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
    #[error("Failed to get object metadata: {0}")]
    HeaderObjectError(#[from] SdkError<HeadObjectError>),
    #[error("Failed to delete object: {0}")]
    DeleteObjectError(#[from] SdkError<DeleteObjectError>),
    #[error("Failed to delete multiple objects: {0}")]
    DeleteObjectsError(#[from] SdkError<DeleteObjectsError>),
    #[error("Failed to delete bucket: {0}")]
    DeleteBucketError(#[from] SdkError<DeleteBucketError>),
    #[error("Bucket is not empty — objects still remain inside")]
    BucketNotEmpty,
    #[error("Missing ETag in upload_part response")]
    MissingETag,
    #[error("Missing upload_id after CreateMultipartUpload")]
    MissingUploadId,
    #[error("I/O error: {0}")]
    IO(#[from] io::Error),
    #[error("ByteStream error: {0}")]
    ByteStreamError(#[from] ByteStreamError),
    #[error("SDK build error: {0}")]
    BuildError(#[from] BuildError),
    #[error("Tokio join error: {0}")]
    TokioJoin(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

impl IntoResponse for S3Error {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self {
            Self::GetObjectError(_) => (StatusCode::NOT_FOUND, "GetObjectError", self.to_string()),
            Self::ListObjectError(_) => (StatusCode::BAD_REQUEST, "ListObjectError", self.to_string()),
            Self::PutObjectError(_) => (StatusCode::BAD_REQUEST, "PutObjectError", self.to_string()),
            Self::CopyObjectError(_) => (StatusCode::BAD_REQUEST, "CopyObjectError", self.to_string()),
            Self::UploadPart(_) => (StatusCode::BAD_REQUEST, "UploadPartError", self.to_string()),
            Self::CreateMultipart(_) => (StatusCode::BAD_REQUEST, "CreateMultipartError", self.to_string()),
            Self::CompleteMultipart(_) => (StatusCode::BAD_REQUEST, "CompleteMultipartError", self.to_string()),
            Self::AbortMultipart(_) => (StatusCode::BAD_REQUEST, "AbortMultipartError", self.to_string()),
            Self::HeaderObjectError(_) => (StatusCode::NOT_FOUND, "HeadObjectError", self.to_string()),
            Self::DeleteObjectError(_) => (StatusCode::BAD_REQUEST, "DeleteObjectError", self.to_string()),
            Self::DeleteObjectsError(_) => (StatusCode::BAD_REQUEST, "DeleteObjectsError", self.to_string()),
            Self::DeleteBucketError(_) => (StatusCode::BAD_REQUEST, "DeleteBucketError", self.to_string()),
            Self::BucketNotEmpty => (StatusCode::CONFLICT, "BucketNotEmpty", self.to_string()),
            Self::MissingETag => (StatusCode::INTERNAL_SERVER_ERROR, "MissingETag", self.to_string()),
            Self::MissingUploadId => (StatusCode::INTERNAL_SERVER_ERROR, "MissingUploadId", self.to_string()),
            Self::IO(_) => (StatusCode::INTERNAL_SERVER_ERROR, "IOError", self.to_string()),
            Self::ByteStreamError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "ByteStreamError", self.to_string()),
            Self::BuildError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "BuildError", self.to_string()),
            Self::TokioJoin(_) => (StatusCode::INTERNAL_SERVER_ERROR, "TokioJoinError", self.to_string()),
            Self::ConfigError(_) => (StatusCode::BAD_REQUEST, "ConfigError", self.to_string()),
        };

        let body = Json(json!({
            "error": error_type,
            "message": message,
        }));

        (status, body).into_response()
    }
}
