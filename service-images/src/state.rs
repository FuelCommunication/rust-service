use s3_client::S3;
use std::sync::Arc;

use crate::Config;

pub type ServerState = Arc<ServerData>;

pub struct ServerData {
    pub s3: S3,
}

impl ServerData {
    pub async fn new(config: &Config) -> ServerState {
        let bucket: &'static str = Box::leak(config.s3.bucket.clone().into_boxed_str());
        let s3 = S3::new(
            config.s3.access_key.clone(),
            config.s3.secret_key.clone(),
            config.s3.region.clone(),
            config.s3.endpoint_url.clone(),
            bucket,
        )
        .await;

        Arc::new(ServerData { s3 })
    }
}
