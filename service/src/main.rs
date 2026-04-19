use service::ServerBuilder;

#[tokio::main]
async fn main() {
    use axum::http::{Method, header};
    dotenvy::dotenv().unwrap();

    ServerBuilder::new()
        .await
        .with_cors(
            [Method::GET, Method::POST, Method::DELETE],
            [header::CONTENT_TYPE, header::ACCEPT],
        )
        .with_tracing()
        .with_prometheus()
        .run()
        .await;
}
