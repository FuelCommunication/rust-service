use service_images::{ServerBuilder, config::Config};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    use axum::http::{Method, header};
    dotenvy::dotenv()?;

    let config = Config::from_env();
    ServerBuilder::new(config)
        .await
        .with_cors(
            [Method::GET, Method::POST, Method::DELETE],
            [header::CONTENT_TYPE, header::ACCEPT],
        )
        .with_tracing()
        .with_prometheus()
        .run()
        .await?;

    Ok(())
}
