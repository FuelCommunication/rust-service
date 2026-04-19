pub mod error;

use aws_config::Region;
use aws_sdk_s3::{
    Client,
    config::Credentials,
    operation::complete_multipart_upload::CompleteMultipartUploadOutput,
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
    types::{Delete, ObjectIdentifier},
};
use error::{S3Error, S3Result};
use std::{borrow::Cow, ffi::OsStr, fmt::Display, path::Path};
use tokio::{fs::File, io::AsyncReadExt as _};

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
        let creds = Credentials::from_keys(access_key, secret_key, None);
        let cfg = aws_config::from_env()
            .endpoint_url(endpoint_url)
            .region(Region::new(region))
            .credentials_provider(creds)
            .load()
            .await;

        let client = Client::new(&cfg);
        Self {
            client,
            bucket: bucket.into(),
        }
    }

    pub async fn copy_object(
        &self,
        destination_bucket: impl Into<String> + Display,
        source_object: impl Into<String> + Display,
        destination_object: impl Into<String> + Display,
    ) -> S3Result<()> {
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

        tracing::info!(
            "Copied from {} to {}/{} with e_tag {}",
            source_key,
            destination_bucket,
            destination_object,
            response
                .copy_object_result
                .unwrap_or_else(|| aws_sdk_s3::types::CopyObjectResult::builder().build())
                .e_tag()
                .unwrap_or("missing")
        );
        Ok(())
    }

    pub async fn upload(
        &self,
        key: impl Into<String>,
        file_name: impl AsRef<OsStr>,
        content_type: impl Into<String>,
    ) -> S3Result<()> {
        let path = Path::new(file_name.as_ref());
        let body = ByteStream::from_path(path).await?;

        self.client
            .put_object()
            .bucket(self.bucket)
            .content_type(content_type)
            .key(key)
            .body(body)
            .send()
            .await?;

        Ok(())
    }

    pub async fn download(
        &self,
        key: impl Into<String>,
        file_name: impl AsRef<Path>,
    ) -> S3Result<()> {
        let response = self
            .client
            .get_object()
            .bucket(self.bucket)
            .key(key)
            .send()
            .await?;

        let mut file = File::create(file_name).await?;
        let mut stream = response.body.into_async_read();
        tokio::io::copy(&mut stream, &mut file).await?;
        Ok(())
    }

    pub async fn delete_object(&self, key: impl Into<String>) -> S3Result<()> {
        self.client
            .delete_object()
            .bucket(self.bucket)
            .key(key)
            .send()
            .await?;

        Ok(())
    }

    pub async fn list_objects(&self, max_keys: Option<i32>) -> S3Result<Vec<String>> {
        let mut list_objects = Vec::new();
        let max_keys = max_keys.unwrap_or(10);
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
                    tracing::error!("{err:?}")
                }
            }
        }

        Ok(list_objects)
    }

    /// Delete the objects in a bucket
    pub async fn delete_objects(&self, objects_to_delete: Vec<String>) -> S3Result<()> {
        // Push into a mut vector to use `?` early return errors while building object keys.
        let mut delete_object_ids = Vec::new();
        for obj in objects_to_delete {
            let obj_id = ObjectIdentifier::builder().key(obj).build()?;
            delete_object_ids.push(obj_id);
        }

        let delete = Delete::builder()
            .set_objects(Some(delete_object_ids))
            .build()?;

        self.client
            .delete_objects()
            .bucket(self.bucket)
            .delete(delete)
            .send()
            .await?;
        Ok(())
    }

    pub async fn clear_bucket(&self) -> S3Result<Vec<String>> {
        let objects = self
            .client
            .list_objects_v2()
            .bucket(self.bucket)
            .send()
            .await?;

        let objects_to_delete = objects
            .contents()
            .iter()
            .filter_map(|obj| obj.key())
            .map(String::from)
            .collect::<Vec<String>>();

        if objects_to_delete.is_empty() {
            return Ok(vec![]);
        }

        let return_keys = objects_to_delete.clone();
        self.delete_objects(objects_to_delete).await?;
        let objects = self
            .client
            .list_objects_v2()
            .bucket(self.bucket)
            .send()
            .await?;

        match objects.key_count {
            Some(0) => Ok(return_keys),
            _ => Err(S3Error::BucketNotEmpty),
        }
    }

    pub async fn delete_bucket(self) -> S3Result<()> {
        let resp = self.client.delete_bucket().bucket(self.bucket).send().await;

        match resp {
            Ok(_) => Ok(()),
            Err(err) => {
                if err
                    .as_service_error()
                    .and_then(aws_sdk_s3::error::ProvideErrorMetadata::code)
                    == Some("NoSuchBucket")
                {
                    Ok(())
                } else {
                    Err(S3Error::DeleteBucketError(err))
                }
            }
        }
    }

    async fn start_multipart_upload(
        &self,
        key: impl Into<String>,
        content_type: impl Into<String>,
    ) -> S3Result<String> {
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

    async fn complete_multipart_upload(
        &self,
        key: impl Into<String>,
        upload_id: impl Into<String>,
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

        let upload = CompletedMultipartUpload::builder()
            .set_parts(Some(upload_parts))
            .build();

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

    async fn upload_part(
        &self,
        key: impl Into<String>,
        upload_id: impl Into<String>,
        part_number: i32,
        stream: ByteStream,
    ) -> S3Result<(i32, String)> {
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

    pub async fn abort_multipart_upload(
        &self,
        key: impl Into<String>,
        upload_id: impl Into<String>,
    ) -> S3Result<()> {
        self.client
            .abort_multipart_upload()
            .bucket(self.bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await?;

        Ok(())
    }

    pub async fn upload_multipart(
        &self,
        key: impl Into<String>,
        file_path: impl AsRef<Path>,
        content_type: impl Into<String>,
        chunk_size: usize,
    ) -> S3Result<()> {
        let key = key.into();
        let upload_id = self.start_multipart_upload(&key, content_type).await?;
        let mut parts: Vec<(i32, String)> = vec![];

        let mut file = File::open(file_path).await?;
        let mut buffer = vec![0u8; chunk_size];
        let mut part_number = 1;

        loop {
            let read_bytes = file.read(&mut buffer).await?;
            if read_bytes == 0 {
                break;
            }

            let data = buffer[..read_bytes].to_vec();
            match self
                .upload_part(&key, &upload_id, part_number, data.into())
                .await
            {
                Ok((part_num, e_tag)) => {
                    parts.push((part_num, e_tag));
                }
                Err(err) => {
                    tracing::error!("Upload part {part_number} failed: {err:?}");
                    self.abort_multipart_upload(&key, &upload_id).await?;
                }
            }
            part_number += 1;
        }

        self.complete_multipart_upload(key, &upload_id, parts)
            .await?;
        Ok(())
    }

    pub async fn download_multipart(
        &self,
        key: impl Into<String>,
        file_path: impl AsRef<Path>,
        chunk_size: usize,
    ) -> S3Result<()> {
        let key = key.into();
        let head_resp = self
            .client
            .head_object()
            .bucket(self.bucket)
            .key(&key)
            .send()
            .await?;

        let total_size = head_resp.content_length().unwrap_or_default() as usize;
        if total_size == 0 {
            File::create(file_path).await?;
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
                Ok((range_start, data.into_bytes().to_vec()))
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
        let mut file = File::create(file_path).await?;
        for (_, chunk) in parts {
            tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        }

        Ok(())
    }
}
