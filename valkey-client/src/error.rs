#[derive(Debug, thiserror::Error)]
pub enum ValkeyError {
    #[error("Redis protocol error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

pub type ValkeyResult<T> = Result<T, ValkeyError>;
