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
