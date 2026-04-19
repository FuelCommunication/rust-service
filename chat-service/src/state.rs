use s3::S3;
use std::sync::Arc;

pub struct ServerData {
    pub s3: S3,
}

pub type ServerState = Arc<ServerData>;
