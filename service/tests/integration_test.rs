use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use service::ServerBuilder;
use tower::ServiceExt;

#[tokio::test]
async fn test_ping() {
    dotenvy::dotenv().ok();
    let app = ServerBuilder::init_router().await;

    let response = app
        .oneshot(Request::builder().uri("/ping").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice::<Value>(&body).unwrap();
    assert_eq!(body, json!({"ping": "pong!"}));
}

#[tokio::test]
async fn test_not_found() {
    dotenvy::dotenv().ok();
    let app = ServerBuilder::init_router().await;
    let response = app
        .oneshot(Request::builder().uri("/does-not-exist").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
