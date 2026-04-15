use axum_test::{
    TestServer,
    multipart::{MultipartForm, Part},
};
use s3_client::S3;
use service_images::{
    ServerBuilder,
    state::{ServerData, ServerState},
};
use std::sync::Arc;
use testcontainers_modules::{
    kafka::Kafka,
    minio::MinIO,
    testcontainers::{ContainerAsync, runners::AsyncRunner as _},
};

const ACCESS_KEY: &str = "minioadmin";
const SECRET_KEY: &str = "minioadmin";
const REGION: &str = "us-east-1";
const BUCKET: &str = "test-images";
const KAFKA_TOPIC: &str = "images-test";

struct TestContext {
    server: TestServer,
    _minio: ContainerAsync<MinIO>,
    _kafka: ContainerAsync<Kafka>,
}

async fn setup() -> anyhow::Result<TestContext> {
    let (minio, kafka) = tokio::join!(MinIO::default().start(), Kafka::default().start());
    let minio = minio?;
    let kafka = kafka?;
    let minio_port = minio.get_host_port_ipv4(9000).await?;
    let endpoint = format!("http://127.0.0.1:{}", minio_port);
    let bucket: &'static str = Box::leak(BUCKET.to_string().into_boxed_str());
    let s3 = S3::new(ACCESS_KEY, SECRET_KEY, REGION, &endpoint, bucket).await;
    s3.create_bucket().await?;
    let kafka_host = kafka.get_host().await?;
    let kafka_port = kafka.get_host_port_ipv4(9093).await?;
    let brokers = format!("{}:{}", kafka_host, kafka_port);

    let state: ServerState = Arc::new(ServerData { s3 });

    let router = ServerBuilder::init_router(state);
    let server = TestServer::new(router);

    Ok(TestContext {
        server,
        _minio: minio,
        _kafka: kafka,
    })
}

#[tokio::test]
async fn test_ping() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let response = ctx.server.get("/ping").await;

    response.assert_status_ok();
    response.assert_json(&serde_json::json!({"ping": "pong!"}));
    Ok(())
}

#[tokio::test]
async fn test_not_found_fallback() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let response = ctx.server.get("/nonexistent").await;

    response.assert_status_not_found();
    Ok(())
}

#[tokio::test]
async fn test_upload_jpeg_success() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();

    let part = Part::bytes(vec![0xFF, 0xD8, 0xFF, 0xE0])
        .file_name("test.jpg")
        .mime_type("image/jpeg");
    let form = MultipartForm::new().add_part("file", part);

    let response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", user_id)
        .multipart(form)
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);
    let body: serde_json::Value = response.json();
    assert!(body["filename"].as_str().is_some_and(|s| !s.is_empty()));
    Ok(())
}

#[tokio::test]
async fn test_upload_png_success() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();

    let part = Part::bytes(vec![0x89, 0x50, 0x4E, 0x47])
        .file_name("test.png")
        .mime_type("image/png");
    let form = MultipartForm::new().add_part("file", part);

    let response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", user_id)
        .multipart(form)
        .await;

    response.assert_status(axum::http::StatusCode::CREATED);
    Ok(())
}

#[tokio::test]
async fn test_upload_unsupported_content_type() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();

    let part = Part::bytes(b"not an image".to_vec())
        .file_name("test.txt")
        .mime_type("text/plain");
    let form = MultipartForm::new().add_part("file", part);

    let response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", user_id)
        .multipart(form)
        .await;

    response.assert_status(axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE);
    Ok(())
}

#[tokio::test]
async fn test_upload_invalid_user_id() -> anyhow::Result<()> {
    let ctx = setup().await?;

    let part = Part::bytes(vec![0xFF, 0xD8, 0xFF, 0xE0])
        .file_name("test.jpg")
        .mime_type("image/jpeg");
    let form = MultipartForm::new().add_part("file", part);

    let response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", "not-a-uuid")
        .multipart(form)
        .await;

    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn test_upload_and_download() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();

    let image_data = b"fake png content for testing".to_vec();
    let part = Part::bytes(image_data.clone()).file_name("test.png").mime_type("image/png");
    let form = MultipartForm::new().add_part("file", part);

    let upload_response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", user_id)
        .multipart(form)
        .await;
    upload_response.assert_status(axum::http::StatusCode::CREATED);

    let body: serde_json::Value = upload_response.json();
    let filename = body["filename"].as_str().unwrap();

    let download_response = ctx.server.get(&format!("/images/{}", filename)).await;
    download_response.assert_status_ok();
    assert_eq!(download_response.as_bytes(), image_data.as_slice());

    let content_type = download_response.header("Content-Type").to_str().unwrap().to_string();
    assert_eq!(content_type, "image/png");

    let content_disposition = download_response.header("Content-Disposition").to_str().unwrap().to_string();
    assert!(content_disposition.contains(filename));

    Ok(())
}

#[tokio::test]
async fn test_download_nonexistent() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let response = ctx.server.get("/images/nonexistent-file-id").await;

    response.assert_status_not_found();
    Ok(())
}

#[tokio::test]
async fn test_download_invalid_filename() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let response = ctx.server.get("/images/bad.filename").await;

    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn test_delete_after_upload() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();

    let part = Part::bytes(b"delete me".to_vec())
        .file_name("test.gif")
        .mime_type("image/gif");
    let form = MultipartForm::new().add_part("file", part);

    let upload_response = ctx
        .server
        .post("/images/upload")
        .add_header("X-User-Id", &user_id)
        .multipart(form)
        .await;
    upload_response.assert_status(axum::http::StatusCode::CREATED);

    let body: serde_json::Value = upload_response.json();
    let filename = body["filename"].as_str().unwrap();

    let delete_response = ctx
        .server
        .delete(&format!("/images/{}", filename))
        .add_header("X-User-Id", &user_id)
        .await;
    delete_response.assert_status_ok();

    let delete_body: serde_json::Value = delete_response.json();
    assert_eq!(delete_body["filename"].as_str().unwrap(), filename);

    let download_response = ctx.server.get(&format!("/images/{}", filename)).await;
    download_response.assert_status_not_found();

    Ok(())
}

#[tokio::test]
async fn test_delete_nonexistent() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();
    let response = ctx
        .server
        .delete("/images/does-not-exist-uuid")
        .add_header("X-User-Id", user_id)
        .await;

    response.assert_status_not_found();
    Ok(())
}

#[tokio::test]
async fn test_delete_invalid_filename() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let user_id = uuid::Uuid::now_v7().to_string();
    let response = ctx.server.delete("/images/bad..name").add_header("X-User-Id", user_id).await;

    response.assert_status(axum::http::StatusCode::BAD_REQUEST);
    Ok(())
}
