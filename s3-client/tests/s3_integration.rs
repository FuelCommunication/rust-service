use s3_client::{PutOptions, S3, S3Config, TransferConfig, config::MIN_PART_SIZE, error::S3Error};
use std::{collections::HashMap, time::Duration};
use tempfile::NamedTempFile;
use testcontainers_modules::testcontainers::{
    ContainerAsync, GenericImage, ImageExt as _,
    core::{IntoContainerPort as _, WaitFor, wait::HttpWaitStrategy},
    runners::AsyncRunner as _,
};

const BUCKET: &str = "test-bucket";
const RUSTFS_IMAGE: &str = "rustfs/rustfs";
const RUSTFS_TAG: &str = "1.0.4";
const RUSTFS_PORT: u16 = 9000;
const ACCESS_KEY: &str = "rustfsadmin";
const SECRET_KEY: &str = "rustfsadmin";

async fn start_rustfs() -> anyhow::Result<(ContainerAsync<GenericImage>, String)> {
    let container = GenericImage::new(RUSTFS_IMAGE, RUSTFS_TAG)
        .with_exposed_port(RUSTFS_PORT.tcp())
        .with_wait_for(WaitFor::http(
            HttpWaitStrategy::new("/health")
                .with_port(RUSTFS_PORT.tcp())
                .with_expected_status_code(200u16),
        ))
        .with_env_var("RUSTFS_ACCESS_KEY", ACCESS_KEY)
        .with_env_var("RUSTFS_SECRET_KEY", SECRET_KEY)
        .start()
        .await?;

    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(RUSTFS_PORT.tcp()).await?;
    Ok((container, format!("http://{host}:{port}")))
}

async fn setup_s3() -> anyhow::Result<(ContainerAsync<GenericImage>, S3)> {
    let (container, endpoint) = start_rustfs().await?;
    let config = S3Config::new(BUCKET)
        .endpoint(endpoint)
        .static_credentials(ACCESS_KEY, SECRET_KEY)
        .transfer(TransferConfig {
            part_size: MIN_PART_SIZE,
            concurrency: 3,
        });
    let s3 = S3::new(config).await?;
    s3.create_bucket().await?;

    Ok((container, s3))
}

fn test_data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn test_upload_and_download() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let data = b"hello world".to_vec();
    s3.upload("test.txt", data.clone(), "text/plain").await?;

    let downloaded = s3.download("test.txt").await?;
    assert_eq!(downloaded.data, data);
    assert_eq!(downloaded.meta.content_type.as_deref(), Some("text/plain"));
    assert_eq!(downloaded.meta.size, Some(data.len() as u64));

    Ok(())
}

#[tokio::test]
async fn test_upload_with_options_and_head() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let options = PutOptions {
        content_type: Some("image/png".into()),
        cache_control: Some("max-age=60".into()),
        metadata: HashMap::from([("owner".into(), "alice".into())]),
        ..Default::default()
    };
    s3.upload_with("meta.png", b"png".to_vec(), &options).await?;

    let meta = s3.head("meta.png").await?;
    assert_eq!(meta.content_type.as_deref(), Some("image/png"));
    assert_eq!(meta.cache_control.as_deref(), Some("max-age=60"));
    assert_eq!(meta.metadata.get("owner").map(String::as_str), Some("alice"));

    Ok(())
}

#[tokio::test]
async fn test_not_found_errors() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    assert!(s3.head("missing").await.unwrap_err().is_not_found());
    assert!(s3.download("missing").await.unwrap_err().is_not_found());

    Ok(())
}

#[tokio::test]
async fn test_download_range() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    s3.upload("range.txt", b"0123456789".to_vec(), "text/plain").await?;
    assert_eq!(&s3.download_range("range.txt", 2, Some(4)).await?[..], b"234");
    assert_eq!(&s3.download_range("range.txt", 7, None).await?[..], b"789");

    Ok(())
}

#[tokio::test]
async fn test_object_exists() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    assert!(!s3.object_exists("missing-key").await?);

    s3.upload("exists.txt", b"data".to_vec(), "text/plain").await?;
    assert!(s3.object_exists("exists.txt").await?);

    Ok(())
}

#[tokio::test]
async fn test_delete_object() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    s3.upload("to-delete.txt", b"data".to_vec(), "text/plain").await?;
    assert!(s3.object_exists("to-delete.txt").await?);

    s3.delete_object("to-delete.txt").await?;
    assert!(!s3.object_exists("to-delete.txt").await?);

    Ok(())
}

#[tokio::test]
async fn test_list_keys_with_prefix() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    for key in ["a.txt", "b.txt", "dir/c.txt"] {
        s3.upload(key, b"1".to_vec(), "text/plain").await?;
    }

    let mut all = s3.list_keys(None).await?;
    all.sort();
    assert_eq!(all, ["a.txt", "b.txt", "dir/c.txt"]);

    assert_eq!(s3.list_keys(Some("dir/")).await?, ["dir/c.txt"]);

    Ok(())
}

#[tokio::test]
async fn test_delete_objects_more_than_one_batch() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let keys: Vec<String> = (0..1005).map(|i| format!("batch/{i:04}")).collect();
    for key in &keys {
        s3.upload(key, Vec::new(), "text/plain").await?;
    }

    assert_eq!(s3.delete_objects(keys).await?, 1005);
    assert!(s3.list_keys(None).await?.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_clear_bucket() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    s3.upload("keep.txt", b"1".to_vec(), "text/plain").await?;
    s3.upload("tmp/f1.txt", b"1".to_vec(), "text/plain").await?;
    s3.upload("tmp/f2.txt", b"2".to_vec(), "text/plain").await?;

    assert_eq!(s3.clear_bucket(Some("tmp/")).await?, 2);
    assert_eq!(s3.list_keys(None).await?, ["keep.txt"]);

    assert_eq!(s3.clear_bucket(None).await?, 1);
    assert!(s3.list_keys(None).await?.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_copy_object_with_special_characters() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let source = "папка/file with spaces+plus.txt";
    s3.upload(source, b"copy me".to_vec(), "text/plain").await?;

    let e_tag = s3.copy_object(source, "copied.txt").await?;
    assert!(e_tag.is_some());

    let downloaded = s3.download("copied.txt").await?;
    assert_eq!(downloaded.data, &b"copy me"[..]);

    Ok(())
}

#[tokio::test]
async fn test_presigned_get_and_put() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    s3.upload("signed.txt", b"secret".to_vec(), "text/plain").await?;
    let get = s3.presign_get("signed.txt", Duration::from_secs(60)).await?;
    assert_eq!(get.method, "GET");
    assert!(get.url.contains("X-Amz-Signature"));

    let put = s3
        .presign_put("upload.txt", Some("text/plain"), Duration::from_secs(60))
        .await?;
    assert_eq!(put.method, "PUT");

    assert!(matches!(
        s3.presign_get("x", Duration::from_secs(8 * 24 * 3600)).await,
        Err(S3Error::Presigning(_))
    ));

    Ok(())
}

#[tokio::test]
async fn test_upload_file_small_uses_single_put() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let file = NamedTempFile::new()?;
    std::fs::write(file.path(), b"small")?;
    s3.upload_file(
        "small.bin",
        file.path(),
        &PutOptions::content_type("application/octet-stream"),
    )
    .await?;

    assert_eq!(s3.download("small.bin").await?.data, &b"small"[..]);

    Ok(())
}

#[tokio::test]
async fn test_upload_file_empty() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let file = NamedTempFile::new()?;
    s3.upload_file("empty.bin", file.path(), &PutOptions::default()).await?;
    assert_eq!(s3.head("empty.bin").await?.size, Some(0));

    let out = NamedTempFile::new()?;
    s3.download_file("empty.bin", out.path()).await?;
    assert!(std::fs::read(out.path())?.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_multipart_upload_and_download() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;
    let data = test_data(3 * MIN_PART_SIZE + 12345);
    let upload_file = NamedTempFile::new()?;
    std::fs::write(upload_file.path(), &data)?;
    s3.upload_file(
        "multipart.bin",
        upload_file.path(),
        &PutOptions::content_type("application/octet-stream"),
    )
    .await?;

    assert_eq!(s3.head("multipart.bin").await?.size, Some(data.len() as u64));

    let download_file = NamedTempFile::new()?;
    s3.download_file("multipart.bin", download_file.path()).await?;

    let downloaded = std::fs::read(download_file.path())?;
    assert_eq!(downloaded.len(), data.len());
    assert!(downloaded == data);

    Ok(())
}

#[tokio::test]
async fn test_upload_reader_exact_part_size() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let data = test_data(2 * MIN_PART_SIZE);
    s3.upload_reader("exact.bin", &data[..], &PutOptions::default()).await?;

    assert!(s3.download("exact.bin").await?.data == data);

    Ok(())
}

#[tokio::test]
async fn test_download_file_missing_object_removes_file() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    let dir = tempfile::tempdir()?;
    let path = dir.path().join("out.bin");
    let err = s3.download_file("missing", &path).await.unwrap_err();
    assert!(err.is_not_found());
    assert!(!path.exists());

    Ok(())
}

#[tokio::test]
async fn test_invalid_config() {
    let err = S3::new(
        S3Config::new(BUCKET)
            .static_credentials(ACCESS_KEY, SECRET_KEY)
            .transfer(TransferConfig {
                part_size: 1024,
                concurrency: 1,
            }),
    )
    .await
    .unwrap_err();
    assert!(matches!(err, S3Error::InvalidConfig(_)));
}

#[tokio::test]
async fn test_create_and_delete_bucket_idempotent() -> anyhow::Result<()> {
    let (_rustfs, s3) = setup_s3().await?;

    s3.create_bucket().await?;
    s3.delete_bucket().await?;
    s3.delete_bucket().await?;

    Ok(())
}
