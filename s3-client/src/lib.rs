pub mod config;
pub mod error;
mod transfer;

pub use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation, ResponseChecksumValidation, retry::RetryConfig,
        timeout::TimeoutConfig,
    },
    presigning::PresigningConfig,
    primitives::DateTime,
    types::{BucketLocationConstraint, CreateBucketConfiguration, Delete, Object, ObjectIdentifier},
};
use bytes::Bytes;
pub use config::{AddressingStyle, ChecksumMode, S3Config, S3Credentials, TransferConfig};
use error::{DeleteFailure, S3Error, S3Result, SdkResultExt as _};
use futures_util::{Stream, StreamExt as _, TryStreamExt as _};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, SystemTime},
};

const DELETE_BATCH_SIZE: usize = 1000;

fn is_retryable_delete_error(code: Option<&str>) -> bool {
    matches!(
        code,
        Some("SlowDown" | "ServiceUnavailable" | "InternalError" | "RequestTimeout" | "Throttling" | "ThrottlingException")
    )
}

fn delete_backoff(attempt: u32) -> Duration {
    Duration::from_millis(100u64.saturating_mul(1 << attempt.min(6))).min(Duration::from_secs(5))
}

const COPY_SOURCE_ESCAPE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~')
    .remove(b'/');

fn to_system_time(dt: Option<&DateTime>) -> Option<SystemTime> {
    dt.and_then(|d| SystemTime::try_from(*d).ok())
}

#[derive(Debug, Clone, Default)]
pub struct ObjectMeta {
    pub size: Option<u64>,
    pub content_type: Option<String>,
    pub e_tag: Option<String>,
    pub last_modified: Option<SystemTime>,
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub metadata: HashMap<String, String>,
}

macro_rules! object_meta_from {
    ($($output:ty),+) => {
        $(
            impl From<&$output> for ObjectMeta {
                fn from(o: &$output) -> Self {
                    Self {
                        size: o.content_length().and_then(|l| u64::try_from(l).ok()),
                        content_type: o.content_type().map(str::to_owned),
                        e_tag: o.e_tag().map(str::to_owned),
                        last_modified: to_system_time(o.last_modified()),
                        cache_control: o.cache_control().map(str::to_owned),
                        content_disposition: o.content_disposition().map(str::to_owned),
                        metadata: o.metadata().cloned().unwrap_or_default(),
                    }
                }
            }
        )+
    };
}

object_meta_from!(
    aws_sdk_s3::operation::head_object::HeadObjectOutput,
    aws_sdk_s3::operation::get_object::GetObjectOutput
);

#[derive(Debug, Clone)]
pub struct S3Object {
    pub data: Bytes,
    pub meta: ObjectMeta,
}

#[derive(Debug)]
pub struct S3ObjectStream {
    pub body: ByteStream,
    pub meta: ObjectMeta,
}

#[derive(Debug, Clone)]
pub struct ObjectInfo {
    pub key: String,
    pub size: Option<u64>,
    pub e_tag: Option<String>,
    pub last_modified: Option<SystemTime>,
}

impl ObjectInfo {
    fn from_sdk(o: &Object) -> Option<Self> {
        Some(Self {
            key: o.key()?.to_owned(),
            size: o.size().and_then(|s| u64::try_from(s).ok()),
            e_tag: o.e_tag().map(str::to_owned),
            last_modified: to_system_time(o.last_modified()),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct PutOptions {
    pub content_type: Option<String>,
    pub cache_control: Option<String>,
    pub content_disposition: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl PutOptions {
    pub fn content_type(content_type: impl Into<String>) -> Self {
        Self {
            content_type: Some(content_type.into()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone)]
pub struct PresignedUrl {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
}

impl From<aws_sdk_s3::presigning::PresignedRequest> for PresignedUrl {
    fn from(req: aws_sdk_s3::presigning::PresignedRequest) -> Self {
        Self {
            url: req.uri().to_owned(),
            method: req.method().to_owned(),
            headers: req.headers().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct S3 {
    client: Client,
    bucket: Arc<str>,
    transfer: TransferConfig,
}

impl S3 {
    pub async fn new(config: S3Config) -> S3Result<Self> {
        config.validate()?;

        let region = Region::new(config.region.clone());
        let path_style = config.force_path_style();
        let mut timeouts = TimeoutConfig::builder().connect_timeout(config.connect_timeout);
        if let Some(t) = config.read_timeout {
            timeouts = timeouts.read_timeout(t);
        }
        if let Some(t) = config.operation_attempt_timeout {
            timeouts = timeouts.operation_attempt_timeout(t);
        }
        if let Some(t) = config.operation_timeout {
            timeouts = timeouts.operation_timeout(t);
        }

        let (request_checksum, response_checksum) = match config.checksum {
            ChecksumMode::WhenRequired => (
                RequestChecksumCalculation::WhenRequired,
                ResponseChecksumValidation::WhenRequired,
            ),
            ChecksumMode::WhenSupported => (
                RequestChecksumCalculation::WhenSupported,
                ResponseChecksumValidation::WhenSupported,
            ),
        };

        let mut builder = aws_sdk_s3::config::Builder::new()
            .behavior_version(BehaviorVersion::latest())
            .region(region.clone())
            .retry_config(RetryConfig::standard().with_max_attempts(config.max_attempts))
            .timeout_config(timeouts.build())
            .force_path_style(path_style)
            .request_checksum_calculation(request_checksum)
            .response_checksum_validation(response_checksum);

        if let Some(endpoint) = &config.endpoint_url {
            builder = builder.endpoint_url(endpoint);
        }

        builder = match config.credentials {
            Some(S3Credentials::Static {
                access_key,
                secret_key,
                session_token,
            }) => builder.credentials_provider(Credentials::new(access_key, secret_key, session_token, None, "s3-client")),
            #[cfg(feature = "credentials-chain")]
            Some(S3Credentials::DefaultChain) | None => builder.credentials_provider(
                aws_config::default_provider::credentials::DefaultCredentialsChain::builder()
                    .region(region)
                    .build()
                    .await,
            ),
            #[cfg(not(feature = "credentials-chain"))]
            None => {
                return Err(S3Error::InvalidConfig(
                    "credentials are not set (enable the `credentials-chain` feature to load them from the environment)".into(),
                ));
            }
        };

        tracing::info!(
            bucket = %config.bucket,
            region = %config.region,
            endpoint = ?config.endpoint_url,
            path_style,
            "S3 client initialized"
        );

        Ok(Self {
            client: Client::from_conf(builder.build()),
            bucket: config.bucket.into(),
            transfer: config.transfer,
        })
    }

    pub fn from_client(client: Client, bucket: impl Into<Arc<str>>, transfer: TransferConfig) -> S3Result<Self> {
        transfer.validate()?;
        Ok(Self {
            client,
            bucket: bucket.into(),
            transfer,
        })
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub async fn create_bucket(&self) -> S3Result<()> {
        let mut req = self.client.create_bucket().bucket(&*self.bucket);

        if let Some(region) = self.client.config().region()
            && region.as_ref() != "us-east-1"
        {
            req = req.create_bucket_configuration(
                CreateBucketConfiguration::builder()
                    .location_constraint(BucketLocationConstraint::from(region.as_ref()))
                    .build(),
            );
        }

        match req.send().await.op("CreateBucket") {
            Ok(_) => {
                tracing::info!(bucket = %self.bucket, "Created bucket");
                Ok(())
            }
            Err(e) if matches!(e.code(), Some("BucketAlreadyOwnedByYou")) => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn delete_bucket(&self) -> S3Result<()> {
        match self
            .client
            .delete_bucket()
            .bucket(&*self.bucket)
            .send()
            .await
            .op("DeleteBucket")
        {
            Ok(_) => {
                tracing::info!(bucket = %self.bucket, "Deleted bucket");
                Ok(())
            }
            Err(e) if e.code() == Some("NoSuchBucket") => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn clear_bucket(&self, prefix: Option<&str>) -> S3Result<usize> {
        let mut listing = std::pin::pin!(self.list_stream(prefix));
        let mut batch = Vec::with_capacity(DELETE_BATCH_SIZE);
        let mut deleted = 0;

        while let Some(object) = listing.try_next().await? {
            batch.push(object.key);
            if batch.len() == DELETE_BATCH_SIZE {
                deleted += self.delete_objects(std::mem::take(&mut batch)).await?;
            }
        }
        if !batch.is_empty() {
            deleted += self.delete_objects(batch).await?;
        }

        if std::pin::pin!(self.list_stream(prefix)).next().await.transpose()?.is_some() {
            return Err(S3Error::BucketNotEmpty);
        }

        tracing::info!(bucket = %self.bucket, prefix = ?prefix, deleted, "Cleared bucket");
        Ok(deleted)
    }

    pub async fn head(&self, key: &str) -> S3Result<ObjectMeta> {
        let output = self
            .client
            .head_object()
            .bucket(&*self.bucket)
            .key(key)
            .send()
            .await
            .op("HeadObject")?;
        Ok(ObjectMeta::from(&output))
    }

    pub async fn object_exists(&self, key: &str) -> S3Result<bool> {
        match self.head(key).await {
            Ok(_) => Ok(true),
            Err(e) if e.is_not_found() => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn upload(&self, key: &str, body: impl Into<ByteStream>, content_type: &str) -> S3Result<()> {
        self.upload_with(key, body, &PutOptions::content_type(content_type)).await
    }

    pub async fn upload_with(&self, key: &str, body: impl Into<ByteStream>, options: &PutOptions) -> S3Result<()> {
        self.client
            .put_object()
            .bucket(&*self.bucket)
            .key(key)
            .set_content_type(options.content_type.clone())
            .set_cache_control(options.cache_control.clone())
            .set_content_disposition(options.content_disposition.clone())
            .set_metadata((!options.metadata.is_empty()).then(|| options.metadata.clone()))
            .body(body.into())
            .send()
            .await
            .op("PutObject")?;

        tracing::debug!(bucket = %self.bucket, key, "Uploaded object");
        Ok(())
    }

    pub async fn download(&self, key: &str) -> S3Result<S3Object> {
        let S3ObjectStream { body, meta } = self.download_stream(key).await?;
        let data = body.collect().await?.into_bytes();
        tracing::debug!(bucket = %self.bucket, key, size = data.len(), "Downloaded object");
        Ok(S3Object { data, meta })
    }

    pub async fn download_stream(&self, key: &str) -> S3Result<S3ObjectStream> {
        let output = self
            .client
            .get_object()
            .bucket(&*self.bucket)
            .key(key)
            .send()
            .await
            .op("GetObject")?;
        let meta = ObjectMeta::from(&output);
        Ok(S3ObjectStream { body: output.body, meta })
    }

    pub async fn download_range(&self, key: &str, start: u64, end: Option<u64>) -> S3Result<Bytes> {
        let range = match end {
            Some(end) => format!("bytes={start}-{end}"),
            None => format!("bytes={start}-"),
        };
        let output = self
            .client
            .get_object()
            .bucket(&*self.bucket)
            .key(key)
            .range(range)
            .send()
            .await
            .op("GetObject")?;
        Ok(output.body.collect().await?.into_bytes())
    }

    pub async fn copy_object(&self, source_key: &str, destination_key: &str) -> S3Result<Option<String>> {
        let bucket = self.bucket.clone();
        self.copy_object_to(&bucket, source_key, destination_key).await
    }

    pub async fn copy_object_to(
        &self,
        destination_bucket: &str,
        source_key: &str,
        destination_key: &str,
    ) -> S3Result<Option<String>> {
        let copy_source = format!("{}/{}", self.bucket, utf8_percent_encode(source_key, COPY_SOURCE_ESCAPE));

        let output = self
            .client
            .copy_object()
            .copy_source(copy_source)
            .bucket(destination_bucket)
            .key(destination_key)
            .send()
            .await
            .op("CopyObject")?;

        let e_tag = output.copy_object_result().and_then(|r| r.e_tag()).map(str::to_owned);
        tracing::debug!(
            source = %format!("{}/{source_key}", self.bucket),
            destination = %format!("{destination_bucket}/{destination_key}"),
            "Copied object"
        );
        Ok(e_tag)
    }

    pub async fn delete_object(&self, key: &str) -> S3Result<()> {
        self.client
            .delete_object()
            .bucket(&*self.bucket)
            .key(key)
            .send()
            .await
            .op("DeleteObject")?;
        tracing::debug!(bucket = %self.bucket, key, "Deleted object");
        Ok(())
    }

    pub async fn delete_objects<I, K>(&self, keys: I) -> S3Result<usize>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        let max_attempts = self.client.config().retry_config().map_or(1, |r| r.max_attempts()).max(1);
        let mut deleted = 0;
        let mut failed = Vec::new();

        for chunk in keys.chunks(DELETE_BATCH_SIZE) {
            let mut pending: Vec<&str> = chunk.iter().map(String::as_str).collect();

            for attempt in 1..=max_attempts {
                let errors = self.delete_batch(&pending).await?;
                deleted += pending.len() - errors.len();

                let (retryable, permanent): (Vec<_>, Vec<_>) = errors
                    .into_iter()
                    .partition(|e| attempt < max_attempts && is_retryable_delete_error(e.code.as_deref()));
                failed.extend(permanent);
                if retryable.is_empty() {
                    break;
                }

                tracing::debug!(bucket = %self.bucket, attempt, keys = retryable.len(), "Retrying failed deletes");
                tokio::time::sleep(delete_backoff(attempt)).await;
                pending = chunk
                    .iter()
                    .map(String::as_str)
                    .filter(|k| retryable.iter().any(|e| e.key == *k))
                    .collect();
            }
        }

        if !failed.is_empty() {
            return Err(S3Error::PartialDelete { deleted, failed });
        }
        tracing::debug!(bucket = %self.bucket, deleted, "Deleted objects");
        Ok(deleted)
    }

    async fn delete_batch(&self, keys: &[&str]) -> S3Result<Vec<DeleteFailure>> {
        let ids = keys
            .iter()
            .map(|key| ObjectIdentifier::builder().key(*key).build())
            .collect::<Result<Vec<_>, _>>()?;
        let delete = Delete::builder().set_objects(Some(ids)).quiet(true).build()?;

        let output = self
            .client
            .delete_objects()
            .bucket(&*self.bucket)
            .delete(delete)
            .send()
            .await
            .op("DeleteObjects")?;

        Ok(output
            .errors()
            .iter()
            .map(|e| DeleteFailure {
                key: e.key().unwrap_or_default().to_owned(),
                code: e.code().map(str::to_owned),
                message: e.message().map(str::to_owned),
            })
            .collect())
    }

    pub fn list_stream(&self, prefix: Option<&str>) -> impl Stream<Item = S3Result<ObjectInfo>> + Send + 'static {
        let pages = self
            .client
            .list_objects_v2()
            .bucket(&*self.bucket)
            .set_prefix(prefix.map(str::to_owned))
            .into_paginator()
            .send();

        futures_util::stream::unfold(
            (pages, VecDeque::new(), false),
            |(mut pages, mut buffer, mut done)| async move {
                loop {
                    if let Some(item) = buffer.pop_front() {
                        return Some((Ok(item), (pages, buffer, done)));
                    }
                    if done {
                        return None;
                    }
                    match pages.next().await {
                        Some(Ok(page)) => buffer.extend(page.contents().iter().filter_map(ObjectInfo::from_sdk)),
                        Some(Err(e)) => {
                            done = true;
                            return Some((Err(S3Error::sdk("ListObjectsV2", e)), (pages, buffer, done)));
                        }
                        None => return None,
                    }
                }
            },
        )
    }

    pub async fn list_keys(&self, prefix: Option<&str>) -> S3Result<Vec<String>> {
        self.list_stream(prefix).map_ok(|o| o.key).try_collect().await
    }

    pub async fn presign_get(&self, key: &str, expires_in: Duration) -> S3Result<PresignedUrl> {
        let request = self
            .client
            .get_object()
            .bucket(&*self.bucket)
            .key(key)
            .presigned(PresigningConfig::expires_in(expires_in)?)
            .await
            .op("GetObject")?;
        Ok(request.into())
    }

    pub async fn presign_put(&self, key: &str, content_type: Option<&str>, expires_in: Duration) -> S3Result<PresignedUrl> {
        let request = self
            .client
            .put_object()
            .bucket(&*self.bucket)
            .key(key)
            .set_content_type(content_type.map(str::to_owned))
            .presigned(PresigningConfig::expires_in(expires_in)?)
            .await
            .op("PutObject")?;
        Ok(request.into())
    }
}
