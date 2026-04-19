use stream_service::ServerBuilder;

#[tokio::main]
async fn main() {
    ServerBuilder::new()
        .await
        .init_tracing(tracing::Level::INFO)
        .run()
        .await
}
