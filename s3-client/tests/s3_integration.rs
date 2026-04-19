use s3_client::S3;
use tempfile::NamedTempFile;
use testcontainers_modules::{minio::MinIO, testcontainers::runners::AsyncRunner as _};

const ACCESS_KEY: &str = "minioadmin";
const SECRET_KEY: &str = "minioadmin";
const REGION: &str = "us-east-1";
const BUCKET: &str = "test-bucket";

async fn setup_s3() -> anyhow::Result<(testcontainers_modules::testcontainers::ContainerAsync<MinIO>, S3)> {
    let minio = MinIO::default().start().await?;
    let host = minio.get_host().await?;
    let port = minio.get_host_port_ipv4(9000).await?;
    let endpoint = format!("http://{}:{}", host, port);
    let bucket: &'static str = Box::leak(BUCKET.to_string().into_boxed_str());
    let s3 = S3::new(ACCESS_KEY, SECRET_KEY, REGION, &endpoint, bucket).await;

    let creds = aws_sdk_s3::config::Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "test");
    let cfg = aws_sdk_s3::config::Builder::new()
        .endpoint_url(&endpoint)
        .credentials_provider(creds)
        .region(aws_config::Region::new(REGION))
        .force_path_style(true)
        .behavior_version_latest()
        .build();
    let raw_client = aws_sdk_s3::Client::from_conf(cfg);
    raw_client.create_bucket().bucket(BUCKET).send().await?;

    Ok((minio, s3))
}

#[tokio::test]
async fn test_upload_and_download() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    let data = b"hello world".to_vec();
    s3.upload("test.txt", data.clone(), "text/plain").await?;

    let downloaded = s3.download("test.txt").await?;
    assert_eq!(downloaded.data, data);
    assert_eq!(downloaded.content_type.as_deref(), Some("text/plain"));

    Ok(())
}

#[tokio::test]
async fn test_object_exists() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    assert!(!s3.object_exists("missing-key").await?);

    s3.upload("exists.txt", b"data".to_vec(), "text/plain").await?;
    assert!(s3.object_exists("exists.txt").await?);

    Ok(())
}

#[tokio::test]
async fn test_delete_object() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.upload("to-delete.txt", b"data".to_vec(), "text/plain").await?;
    assert!(s3.object_exists("to-delete.txt").await?);

    s3.delete_object("to-delete.txt").await?;
    assert!(!s3.object_exists("to-delete.txt").await?);

    Ok(())
}

#[tokio::test]
async fn test_list_objects() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.upload("a.txt", b"1".to_vec(), "text/plain").await?;
    s3.upload("b.txt", b"2".to_vec(), "text/plain").await?;
    s3.upload("c.txt", b"3".to_vec(), "text/plain").await?;

    let objects = s3.list_objects(None).await?;
    assert_eq!(objects.len(), 3);
    assert!(objects.contains(&"a.txt".to_string()));
    assert!(objects.contains(&"b.txt".to_string()));
    assert!(objects.contains(&"c.txt".to_string()));

    Ok(())
}

#[tokio::test]
async fn test_delete_objects_batch() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.upload("x.txt", b"1".to_vec(), "text/plain").await?;
    s3.upload("y.txt", b"2".to_vec(), "text/plain").await?;

    let deleted = s3.delete_objects(vec!["x.txt".into(), "y.txt".into()]).await?;
    assert_eq!(deleted, 2);

    let objects = s3.list_objects(None).await?;
    assert!(objects.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_clear_bucket() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.upload("f1.txt", b"1".to_vec(), "text/plain").await?;
    s3.upload("f2.txt", b"2".to_vec(), "text/plain").await?;

    let cleared = s3.clear_bucket().await?;
    assert_eq!(cleared.len(), 2);

    let objects = s3.list_objects(None).await?;
    assert!(objects.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_copy_object() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.upload("original.txt", b"copy me".to_vec(), "text/plain").await?;

    let e_tag = s3.copy_object(BUCKET, "original.txt", "copied.txt").await?;
    assert!(!e_tag.is_empty());

    let downloaded = s3.download("copied.txt").await?;
    assert_eq!(downloaded.data, b"copy me");

    Ok(())
}

#[tokio::test]
async fn test_multipart_upload_and_download() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    let chunk_size = 5 * 1024 * 1024;
    let data: Vec<u8> = (0..3 * chunk_size).map(|i| (i % 256) as u8).collect();

    let upload_file = NamedTempFile::new()?;
    std::fs::write(upload_file.path(), &data)?;

    s3.upload_multipart(
        "multipart.bin",
        upload_file.path(),
        "application/octet-stream",
        Some(chunk_size),
    )
    .await?;

    assert!(s3.object_exists("multipart.bin").await?);

    let download_file = NamedTempFile::new()?;
    s3.download_multipart("multipart.bin", download_file.path(), Some(chunk_size))
        .await?;

    let downloaded = std::fs::read(download_file.path())?;
    assert_eq!(downloaded.len(), data.len());
    assert_eq!(downloaded, data);

    Ok(())
}

#[tokio::test]
async fn test_delete_bucket() -> anyhow::Result<()> {
    let (_minio, s3) = setup_s3().await?;

    s3.delete_bucket().await?;

    Ok(())
}
