pub mod error;

use aws_config::Region;
use aws_sdk_s3::{
    Client,
    config::Credentials,
    error::ProvideErrorMetadata,
    operation::complete_multipart_upload::CompleteMultipartUploadOutput,
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart, Delete, ObjectIdentifier},
};
use error::{S3Error, S3Result};
use std::{borrow::Cow, path::Path};
use tokio::{fs::File, io::AsyncReadExt as _};

const DEFAULT_CHUNK_SIZE: usize = 5 * 1024 * 1024; // 5MB

pub struct S3 {
    client: Client,
    bucket: &'static str,
}

impl S3 {
    pub async fn new(
        access_key: impl Into<String>,
        secret_key: impl Into<String>,
        region: impl Into<Cow<'static, str>>,
        endpoint_url: impl Into<String>,
        bucket: impl Into<&'static str>,
    ) -> Self {
        let creds = Credentials::new(access_key, secret_key, None, None, "loaded-from-custom-env");
        let cfg = aws_sdk_s3::config::Builder::new()
            .endpoint_url(endpoint_url)
            .credentials_provider(creds)
            .region(Region::new(region))
            .force_path_style(true)
            .behavior_version_latest()
            .build();

        let client = Client::from_conf(cfg);
        Self {
            client,
            bucket: bucket.into(),
        }
    }

    pub fn bucket(&self) -> &str {
        self.bucket
    }

    pub async fn object_exists(&self, key: impl Into<String>) -> S3Result<bool> {
        let result = self.client.head_object().bucket(self.bucket).key(key).send().await;

        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                if e.as_service_error().and_then(ProvideErrorMetadata::code) == Some("NotFound") {
                    Ok(false)
                } else {
                    Err(S3Error::HeaderObjectError(e))
                }
            }
        }
    }

    pub async fn copy_object(
        &self,
        destination_bucket: impl Into<String>,
        source_object: impl Into<String>,
        destination_object: impl Into<String>,
    ) -> S3Result<String> {
        let destination_bucket = destination_bucket.into();
        let source_object = source_object.into();
        let destination_object = destination_object.into();
        let source_key = format!("{}/{}", self.bucket, source_object);

        let response = self
            .client
            .copy_object()
            .copy_source(&source_key)
            .bucket(&destination_bucket)
            .key(&destination_object)
            .send()
            .await?;

        let e_tag = response.copy_object_result().and_then(|r| r.e_tag()).unwrap_or("missing");

        tracing::info!(
            source = %source_key,
            destination = %format!("{}/{}", destination_bucket, destination_object),
            e_tag = %e_tag,
            "Successfully copied object"
        );

        Ok(e_tag.to_string())
    }

    pub async fn upload(
        &self,
        key: impl Into<String>,
        body: impl Into<ByteStream>,
        content_type: impl Into<String>,
    ) -> S3Result<()> {
        let key = key.into();
        let body = body.into();
        let size = body.bytes().unwrap_or_default().len();
        let content_type = content_type.into();

        self.client
            .put_object()
            .bucket(self.bucket)
            .content_type(&content_type)
            .key(&key)
            .body(body)
            .send()
            .await?;

        tracing::info!("Uploaded file: key={key}, size={size} bytes, content_type={content_type}");
        Ok(())
    }

    pub async fn download(&self, key: impl Into<String>) -> S3Result<Vec<u8>> {
        let key = key.into();
        let object = self.client.get_object().bucket(self.bucket).key(&key).send().await?;
        let body = object.body.collect().await.map_err(S3Error::from)?.to_vec();
        tracing::info!("File downloaded: {}, size: {} bytes", key, body.len());
        Ok(body)
    }

    pub async fn delete_object(&self, key: impl Into<String>) -> S3Result<()> {
        let key = key.into();
        self.client.delete_object().bucket(self.bucket).key(&key).send().await?;
        tracing::info!("File deleted with key: {key}");
        Ok(())
    }

    pub async fn list_objects(&self, max_keys: Option<i32>) -> S3Result<Vec<String>> {
        let mut list_objects = Vec::with_capacity(max_keys.unwrap_or(10) as usize);
        let max_keys = max_keys.unwrap_or(1000);

        let mut response = self
            .client
            .list_objects_v2()
            .bucket(self.bucket)
            .max_keys(max_keys)
            .into_paginator()
            .send();

        while let Some(result) = response.next().await {
            match result {
                Ok(output) => {
                    for object in output.contents() {
                        list_objects.push(object.key().unwrap_or("Unknown").to_owned());
                    }
                }
                Err(err) => {
                    tracing::error!(error = ?err, "Failed to list objects");
                    return Err(S3Error::ListObjectError(err));
                }
            }
        }

        Ok(list_objects)
    }

    pub async fn delete_objects(&self, keys: Vec<String>) -> S3Result<usize> {
        if keys.is_empty() {
            return Ok(0);
        }

        let mut delete_object_ids = Vec::new();
        for key in &keys {
            let obj_id = ObjectIdentifier::builder().key(key).build()?;
            delete_object_ids.push(obj_id);
        }

        let delete = Delete::builder().set_objects(Some(delete_object_ids)).build()?;

        self.client.delete_objects().bucket(self.bucket).delete(delete).send().await?;

        Ok(keys.len())
    }

    pub async fn clear_bucket(&self) -> S3Result<Vec<String>> {
        let objects = self.list_objects(None).await?;

        if objects.is_empty() {
            return Ok(vec![]);
        }

        let deleted_count = self.delete_objects(objects.clone()).await?;
        tracing::info!(
            bucket = %self.bucket,
            deleted = deleted_count,
            "Cleared bucket"
        );

        let remaining = self.list_objects(Some(1)).await?;
        if !remaining.is_empty() {
            return Err(S3Error::BucketNotEmpty);
        }

        Ok(objects)
    }

    pub async fn delete_bucket(self) -> S3Result<()> {
        let resp = self.client.delete_bucket().bucket(self.bucket).send().await;

        match resp {
            Ok(_) => {
                tracing::info!(bucket = %self.bucket, "Deleted bucket");
                Ok(())
            }
            Err(err) => {
                if err.as_service_error().and_then(ProvideErrorMetadata::code) == Some("NoSuchBucket") {
                    Ok(())
                } else {
                    Err(S3Error::DeleteBucketError(err))
                }
            }
        }
    }

    async fn start_multipart_upload(&self, key: &str, content_type: &str) -> S3Result<String> {
        let response = self
            .client
            .create_multipart_upload()
            .bucket(self.bucket)
            .key(key)
            .content_type(content_type)
            .send()
            .await?;

        let upload_id = response.upload_id().ok_or(S3Error::MissingUploadId)?;
        Ok(upload_id.to_owned())
    }

    async fn upload_part(&self, key: &str, upload_id: &str, part_number: i32, stream: ByteStream) -> S3Result<(i32, String)> {
        let resp = self
            .client
            .upload_part()
            .bucket(self.bucket)
            .key(key)
            .upload_id(upload_id)
            .part_number(part_number)
            .body(stream)
            .send()
            .await?;

        let e_tag = resp.e_tag().ok_or(S3Error::MissingETag)?;
        Ok((part_number, e_tag.to_string()))
    }

    async fn complete_multipart_upload(
        &self,
        key: &str,
        upload_id: &str,
        parts: Vec<(i32, String)>,
    ) -> S3Result<CompleteMultipartUploadOutput> {
        let upload_parts = parts
            .into_iter()
            .map(|(num, tag)| {
                CompletedPart::builder()
                    .set_part_number(Some(num))
                    .set_e_tag(Some(tag))
                    .build()
            })
            .collect::<Vec<_>>();

        let upload = CompletedMultipartUpload::builder().set_parts(Some(upload_parts)).build();
        let result = self
            .client
            .complete_multipart_upload()
            .bucket(self.bucket)
            .key(key)
            .multipart_upload(upload)
            .upload_id(upload_id)
            .send()
            .await?;

        Ok(result)
    }

    pub async fn abort_multipart_upload(&self, key: impl Into<String>, upload_id: impl Into<String>) -> S3Result<()> {
        self.client
            .abort_multipart_upload()
            .bucket(self.bucket)
            .key(key.into())
            .upload_id(upload_id.into())
            .send()
            .await?;
        Ok(())
    }

    pub async fn upload_multipart(
        &self,
        key: impl Into<String>,
        file_path: impl AsRef<Path>,
        content_type: impl Into<String>,
        chunk_size: Option<usize>,
    ) -> S3Result<()> {
        let key = key.into();
        let content_type = content_type.into();
        let chunk_size = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
        let upload_id = self.start_multipart_upload(&key, &content_type).await?;
        let mut parts: Vec<(i32, String)> = vec![];
        let mut file = File::open(&file_path).await?;
        let mut buffer = vec![0u8; chunk_size];
        let mut part_number = 1;

        loop {
            let read_bytes = file.read(&mut buffer).await?;
            if read_bytes == 0 {
                break;
            }

            let data = buffer[..read_bytes].to_vec();
            match self.upload_part(&key, &upload_id, part_number, data.into()).await {
                Ok((part_num, e_tag)) => {
                    tracing::debug!(
                        part = part_num,
                        e_tag = %e_tag,
                        "Uploaded part"
                    );
                    parts.push((part_num, e_tag));
                }
                Err(err) => {
                    tracing::error!(
                        part = part_number,
                        error = ?err,
                        "Upload part failed, aborting"
                    );
                    self.abort_multipart_upload(&key, &upload_id).await?;
                    return Err(err);
                }
            }
            part_number += 1;
        }

        self.complete_multipart_upload(&key, &upload_id, parts).await?;
        tracing::info!(
            key = %key,
            parts = part_number - 1,
            "Completed multipart upload"
        );

        Ok(())
    }

    pub async fn download_multipart(
        &self,
        key: impl Into<String>,
        file_path: impl AsRef<Path>,
        chunk_size: Option<usize>,
    ) -> S3Result<()> {
        let key = key.into();
        let chunk_size = chunk_size.unwrap_or(DEFAULT_CHUNK_SIZE);
        let head_resp = self.client.head_object().bucket(self.bucket).key(&key).send().await?;
        let total_size = head_resp.content_length().unwrap_or_default() as usize;

        if total_size == 0 {
            File::create(&file_path).await?;
            return Ok(());
        }

        let mut handles = Vec::new();
        let mut start: usize = 0;

        while start < total_size {
            let end = std::cmp::min(start + chunk_size, total_size) - 1;
            let client = self.client.clone();
            let bucket = self.bucket;
            let key = key.clone();
            let range_start = start;

            let handle = tokio::spawn(async move {
                let resp = client
                    .get_object()
                    .bucket(bucket)
                    .key(&key)
                    .range(format!("bytes={range_start}-{end}"))
                    .send()
                    .await?;

                let data = resp.body.collect().await?;
                Ok::<_, S3Error>((range_start, data.into_bytes().to_vec()))
            });

            handles.push(handle);
            start += chunk_size;
        }

        let mut parts: Vec<(usize, Vec<u8>)> = Vec::new();
        for handle in handles {
            match handle.await {
                Ok(Ok((offset, chunk))) => parts.push((offset, chunk)),
                Ok(Err(e)) => return Err(e),
                Err(join_err) => return Err(S3Error::TokioJoin(join_err.to_string())),
            }
        }

        parts.sort_by_key(|(offset, _)| *offset);
        let mut file = File::create(&file_path).await?;
        for (_, chunk) in parts {
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }

        tracing::info!(
            key = %key,
            size = total_size,
            "Downloaded file using multipart"
        );

        Ok(())
    }
}
