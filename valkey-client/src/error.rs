pub type ValkeyResult<T> = Result<T, ValkeyError>;

#[derive(Debug, thiserror::Error)]
pub enum ValkeyError {
    #[error("Valkey error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Failed to encode value for key `{key}`: {source}")]
    Encode {
        key: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("Failed to decode value of key `{key}`: {source}")]
    Decode {
        key: String,
        #[source]
        source: serde_json::Error,
    },
}

impl ValkeyError {
    pub(crate) fn encode(key: &str, source: serde_json::Error) -> Self {
        Self::Encode {
            key: key.to_owned(),
            source,
        }
    }

    pub(crate) fn decode(key: &str, source: serde_json::Error) -> Self {
        Self::Decode {
            key: key.to_owned(),
            source,
        }
    }

    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Redis(e) if e.is_timeout())
    }

    pub fn is_connection_error(&self) -> bool {
        matches!(self, Self::Redis(e) if e.is_connection_dropped() || e.is_connection_refusal() || e.is_io_error())
    }

    pub fn is_decode(&self) -> bool {
        matches!(self, Self::Decode { .. })
    }
}
