use crate::{
    ByteStream, PutOptions, S3,
    config::MAX_PARTS,
    error::{S3Error, S3Result, SdkResultExt as _},
};
use aws_sdk_s3::{
    Client,
    types::{CompletedMultipartUpload, CompletedPart},
};
use bytes::Bytes;
use std::{io::SeekFrom, path::Path, sync::Arc};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncRead, AsyncReadExt as _, AsyncSeekExt as _, AsyncWriteExt as _},
    task::JoinSet,
};

async fn read_chunk<R: AsyncRead + Unpin>(reader: &mut R, size: usize) -> S3Result<Bytes> {
    let mut buf = vec![0u8; size];
    let mut filled = 0;
    while filled < size {
        let n = reader.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(Bytes::from(buf))
}

async fn write_stream(file: &mut File, mut body: ByteStream) -> S3Result<()> {
    while let Some(chunk) = body.try_next().await? {
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    Ok(())
}

impl S3 {
    pub async fn upload_file(&self, key: &str, path: impl AsRef<Path>, options: &PutOptions) -> S3Result<()> {
        let file = File::open(path).await?;
        self.upload_reader(key, file, options).await
    }

    pub async fn upload_reader<R>(&self, key: &str, mut reader: R, options: &PutOptions) -> S3Result<()>
    where
        R: AsyncRead + Unpin + Send,
    {
        let part_size = self.transfer.part_size;
        let first = read_chunk(&mut reader, part_size).await?;
        if first.len() < part_size {
            return self.upload_with(key, first, options).await;
        }

        let upload_id = self.start_multipart_upload(key, options).await?;
        let result = async {
            let parts = self.upload_parts(key, &upload_id, first, &mut reader).await?;
            let part_count = parts.len();
            self.complete_multipart_upload(key, &upload_id, parts).await?;
            Ok::<_, S3Error>(part_count)
        }
        .await;

        match result {
            Ok(parts) => {
                tracing::debug!(bucket = %self.bucket, key, parts, "Completed multipart upload");
                Ok(())
            }
            Err(err) => {
                tracing::warn!(bucket = %self.bucket, key, error = %err, "Multipart upload failed, aborting");
                if let Err(abort_err) = self.abort_multipart_upload(key, &upload_id).await {
                    tracing::error!(bucket = %self.bucket, key, upload_id, error = %abort_err, "Failed to abort multipart upload");
                }
                Err(err)
            }
        }
    }

    pub async fn abort_multipart_upload(&self, key: &str, upload_id: &str) -> S3Result<()> {
        self.client
            .abort_multipart_upload()
            .bucket(&*self.bucket)
            .key(key)
            .upload_id(upload_id)
            .send()
            .await
            .op("AbortMultipartUpload")?;
        Ok(())
    }

    async fn start_multipart_upload(&self, key: &str, options: &PutOptions) -> S3Result<String> {
        let output = self
            .client
            .create_multipart_upload()
            .bucket(&*self.bucket)
            .key(key)
            .set_content_type(options.content_type.clone())
            .set_cache_control(options.cache_control.clone())
            .set_content_disposition(options.content_disposition.clone())
            .set_metadata((!options.metadata.is_empty()).then(|| options.metadata.clone()))
            .send()
            .await
            .op("CreateMultipartUpload")?;
        output.upload_id().map(str::to_owned).ok_or(S3Error::MissingUploadId)
    }

    async fn upload_parts<R>(&self, key: &str, upload_id: &str, first: Bytes, reader: &mut R) -> S3Result<Vec<CompletedPart>>
    where
        R: AsyncRead + Unpin + Send,
    {
        let mut tasks: JoinSet<S3Result<CompletedPart>> = JoinSet::new();
        let mut parts = Vec::new();
        let mut part_number = 1;
        let mut chunk = first;

        loop {
            if part_number > MAX_PARTS {
                return Err(S3Error::TooManyParts);
            }
            if tasks.len() >= self.transfer.concurrency
                && let Some(done) = tasks.join_next().await
            {
                parts.push(done??);
            }

            tasks.spawn(upload_part(
                self.client.clone(),
                self.bucket.clone(),
                key.to_owned(),
                upload_id.to_owned(),
                part_number,
                chunk,
            ));
            part_number += 1;

            chunk = read_chunk(reader, self.transfer.part_size).await?;
            if chunk.is_empty() {
                break;
            }
        }

        while let Some(done) = tasks.join_next().await {
            parts.push(done??);
        }
        parts.sort_by_key(|p| p.part_number());
        Ok(parts)
    }

    async fn complete_multipart_upload(&self, key: &str, upload_id: &str, parts: Vec<CompletedPart>) -> S3Result<()> {
        self.client
            .complete_multipart_upload()
            .bucket(&*self.bucket)
            .key(key)
            .upload_id(upload_id)
            .multipart_upload(CompletedMultipartUpload::builder().set_parts(Some(parts)).build())
            .send()
            .await
            .op("CompleteMultipartUpload")?;
        Ok(())
    }

    pub async fn download_file(&self, key: &str, path: impl AsRef<Path>) -> S3Result<()> {
        let path = path.as_ref();
        let result = self.download_file_inner(key, path).await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(path).await;
        }
        result
    }

    async fn download_file_inner(&self, key: &str, path: &Path) -> S3Result<()> {
        let meta = self.head(key).await?;
        let size = meta.size.ok_or(S3Error::MissingContentLength)?;
        let part_size = self.transfer.part_size as u64;

        if size <= part_size {
            let stream = self.download_stream(key).await?;
            let mut file = File::create(path).await?;
            return write_stream(&mut file, stream.body).await;
        }

        File::create(path).await?.set_len(size).await?;

        let path: Arc<Path> = Arc::from(path);
        let mut tasks: JoinSet<S3Result<()>> = JoinSet::new();
        let mut start = 0;

        while start < size {
            if tasks.len() >= self.transfer.concurrency
                && let Some(done) = tasks.join_next().await
            {
                done??;
            }
            let end = (start + part_size).min(size) - 1;
            tasks.spawn(download_range_to_file(
                self.client.clone(),
                self.bucket.clone(),
                key.to_owned(),
                meta.e_tag.clone(),
                start,
                end,
                path.clone(),
            ));
            start = end + 1;
        }

        while let Some(done) = tasks.join_next().await {
            done??;
        }

        tracing::debug!(bucket = %self.bucket, key, size, "Downloaded object to file");
        Ok(())
    }
}

async fn upload_part(
    client: Client,
    bucket: Arc<str>,
    key: String,
    upload_id: String,
    part_number: i32,
    data: Bytes,
) -> S3Result<CompletedPart> {
    let output = client
        .upload_part()
        .bucket(&*bucket)
        .key(key)
        .upload_id(upload_id)
        .part_number(part_number)
        .body(ByteStream::from(data))
        .send()
        .await
        .op("UploadPart")?;

    let e_tag = output.e_tag().ok_or(S3Error::MissingETag)?;
    tracing::trace!(part_number, e_tag, "Uploaded part");
    Ok(CompletedPart::builder().part_number(part_number).e_tag(e_tag).build())
}

async fn download_range_to_file(
    client: Client,
    bucket: Arc<str>,
    key: String,
    e_tag: Option<String>,
    start: u64,
    end: u64,
    path: Arc<Path>,
) -> S3Result<()> {
    let output = client
        .get_object()
        .bucket(&*bucket)
        .key(key)
        .range(format!("bytes={start}-{end}"))
        .set_if_match(e_tag)
        .send()
        .await
        .op("GetObject")?;

    let mut file = OpenOptions::new().write(true).open(&*path).await?;
    file.seek(SeekFrom::Start(start)).await?;
    write_stream(&mut file, output.body).await
}
