use aws_sdk_s3::{
    config::http::HttpResponse,
    error::{BuildError, DisplayErrorContext, ProvideErrorMetadata, SdkError},
    presigning::PresigningConfigError,
    primitives::ByteStreamError,
};
use std::io;
use tokio::task::JoinError;

pub type S3Result<T> = Result<T, S3Error>;

#[derive(Debug, Clone)]
pub struct DeleteFailure {
    pub key: String,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum S3Error {
    #[error("S3 operation `{op}` failed: {}", DisplayErrorContext(source.as_ref()))]
    Sdk {
        op: &'static str,
        status: Option<u16>,
        #[source]
        source: Box<aws_sdk_s3::Error>,
    },
    #[error("Failed to delete {} objects ({deleted} deleted)", failed.len())]
    PartialDelete { deleted: usize, failed: Vec<DeleteFailure> },
    #[error("Objects still remain after clearing")]
    BucketNotEmpty,
    #[error("Missing ETag in upload_part response")]
    MissingETag,
    #[error("Missing upload_id after CreateMultipartUpload")]
    MissingUploadId,
    #[error("Missing Content-Length in response")]
    MissingContentLength,
    #[error("Multipart upload exceeds the maximum of {} parts", crate::config::MAX_PARTS)]
    TooManyParts,
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("ByteStream error: {0}")]
    ByteStream(#[from] ByteStreamError),
    #[error("SDK build error: {0}")]
    Build(#[from] BuildError),
    #[error("Presigning config error: {0}")]
    Presigning(#[from] PresigningConfigError),
    #[error("Background task failed: {0}")]
    Join(#[from] JoinError),
}

impl S3Error {
    pub(crate) fn sdk<E>(op: &'static str, err: SdkError<E, HttpResponse>) -> Self
    where
        aws_sdk_s3::Error: From<SdkError<E, HttpResponse>>,
    {
        let status = err.raw_response().map(|r| r.status().as_u16());
        Self::Sdk {
            op,
            status,
            source: Box::new(err.into()),
        }
    }

    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Sdk { status, .. } => *status,
            _ => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Sdk { source, .. } => source.code(),
            _ => None,
        }
    }

    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404) || matches!(self.code(), Some("NoSuchKey" | "NotFound" | "NoSuchBucket"))
    }

    pub fn is_access_denied(&self) -> bool {
        self.status() == Some(403) || matches!(self.code(), Some("AccessDenied" | "Forbidden"))
    }

    pub fn is_precondition_failed(&self) -> bool {
        self.status() == Some(412) || self.code() == Some("PreconditionFailed")
    }
}

pub(crate) trait SdkResultExt<T> {
    fn op(self, op: &'static str) -> S3Result<T>;
}

impl<T, E> SdkResultExt<T> for Result<T, SdkError<E, HttpResponse>>
where
    aws_sdk_s3::Error: From<SdkError<E, HttpResponse>>,
{
    fn op(self, op: &'static str) -> S3Result<T> {
        self.map_err(|e| S3Error::sdk(op, e))
    }
}
