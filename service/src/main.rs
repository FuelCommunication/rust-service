use service::ServerBuilder;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().unwrap();

    ServerBuilder::new()
        .await
        .init_tracing()
        .init_cors()
        .run()
        .await;
}
