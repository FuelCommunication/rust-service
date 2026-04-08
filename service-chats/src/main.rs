use service_chats::{Config, ServerBuilder};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use axum::http::{Method, header};
    dotenvy::dotenv()?;

    let config = Config::from_env();
    ServerBuilder::new(config)
        .await
        .with_cors([Method::GET], [header::CONTENT_TYPE, header::ACCEPT])
        .with_tracing()
        .with_prometheus()
        .run()
        .await?;

    Ok(())
}
