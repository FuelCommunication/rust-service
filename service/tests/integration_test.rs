use axum_test::TestServer;
use serde_json::json;
use service::ServerBuilder;

#[tokio::test]
async fn test_ping() {
    let app = ServerBuilder::init_router().await;
    let server = TestServer::new(app).unwrap();
    let response = server
        .get("/ping")
        .await;

    response.assert_status_ok();
    response.assert_json(&json!({"ping": "pong!"}));
}

#[tokio::test]
async fn test_not_found() {
    let app = ServerBuilder::init_router().await;
    let server = TestServer::new(app).unwrap();
    let response = server
        .get("/does-not-exist")
        .await;

    response.assert_status_not_found();
}
