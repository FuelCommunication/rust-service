use aws_sdk_s3::{
    error::{BuildError, SdkError},
    operation::{
        abort_multipart_upload::AbortMultipartUploadError, complete_multipart_upload::CompleteMultipartUploadError,
        copy_object::CopyObjectError, create_bucket::CreateBucketError, create_multipart_upload::CreateMultipartUploadError,
        delete_bucket::DeleteBucketError, delete_object::DeleteObjectError, delete_objects::DeleteObjectsError,
        get_object::GetObjectError, head_object::HeadObjectError, list_objects_v2::ListObjectsV2Error,
        put_object::PutObjectError, upload_part::UploadPartError,
    },
    primitives::ByteStreamError,
};
use std::io;

pub type S3Result<T> = Result<T, S3Error>;

macro_rules! impl_boxed_from {
    ($err:ty { $($variant:ident <= $src:ty),+ $(,)? }) => {
        $(
            impl From<$src> for $err {
                fn from(e: $src) -> Self {
                    Self::$variant(Box::new(e))
                }
            }
        )+
    };
}

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("Failed to get object from S3: {0}")]
    GetObjectError(Box<SdkError<GetObjectError>>),
    #[error("Failed to list objects in bucket: {0}")]
    ListObjectError(Box<SdkError<ListObjectsV2Error>>),
    #[error("Failed to put object to S3: {0}")]
    PutObjectError(Box<SdkError<PutObjectError>>),
    #[error("Failed to copy object: {0}")]
    CopyObjectError(Box<SdkError<CopyObjectError>>),
    #[error("Failed to upload part: {0}")]
    UploadPart(Box<SdkError<UploadPartError>>),
    #[error("Failed to create multipart upload: {0}")]
    CreateMultipart(Box<SdkError<CreateMultipartUploadError>>),
    #[error("Failed to complete multipart upload: {0}")]
    CompleteMultipart(Box<SdkError<CompleteMultipartUploadError>>),
    #[error("Failed to abort multipart upload: {0}")]
    AbortMultipart(Box<SdkError<AbortMultipartUploadError>>),
    #[error("Failed to get object metadata: {0}")]
    HeaderObjectError(Box<SdkError<HeadObjectError>>),
    #[error("Failed to delete object: {0}")]
    DeleteObjectError(Box<SdkError<DeleteObjectError>>),
    #[error("Failed to delete multiple objects: {0}")]
    DeleteObjectsError(Box<SdkError<DeleteObjectsError>>),
    #[error("Failed to create bucket: {0}")]
    CreateBucketError(Box<SdkError<CreateBucketError>>),
    #[error("Failed to delete bucket: {0}")]
    DeleteBucketError(Box<SdkError<DeleteBucketError>>),
    #[error("Bucket is not empty - objects still remain inside")]
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

impl_boxed_from!(S3Error {
    GetObjectError <= SdkError<GetObjectError>,
    ListObjectError <= SdkError<ListObjectsV2Error>,
    PutObjectError <= SdkError<PutObjectError>,
    CopyObjectError <= SdkError<CopyObjectError>,
    UploadPart <= SdkError<UploadPartError>,
    CreateMultipart <= SdkError<CreateMultipartUploadError>,
    CompleteMultipart <= SdkError<CompleteMultipartUploadError>,
    AbortMultipart <= SdkError<AbortMultipartUploadError>,
    HeaderObjectError <= SdkError<HeadObjectError>,
    DeleteObjectError <= SdkError<DeleteObjectError>,
    DeleteObjectsError <= SdkError<DeleteObjectsError>,
    CreateBucketError <= SdkError<CreateBucketError>,
    DeleteBucketError <= SdkError<DeleteBucketError>,
});
