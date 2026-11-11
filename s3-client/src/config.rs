use crate::error::{S3Error, S3Result};
use std::{fmt, time::Duration};

pub const MIN_PART_SIZE: usize = 5 * 1024 * 1024;
pub const MAX_PART_SIZE: usize = 5 * 1024 * 1024 * 1024;
pub const MAX_PARTS: i32 = 10_000;

#[derive(Clone)]
pub enum S3Credentials {
    Static {
        access_key: String,
        secret_key: String,
        session_token: Option<String>,
    },
    #[cfg(feature = "credentials-chain")]
    DefaultChain,
}

impl fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static { access_key, .. } => f
                .debug_struct("Static")
                .field("access_key", access_key)
                .field("secret_key", &"<redacted>")
                .finish_non_exhaustive(),
            #[cfg(feature = "credentials-chain")]
            Self::DefaultChain => f.write_str("DefaultChain"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressingStyle {
    Path,
    VirtualHosted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumMode {
    WhenRequired,
    WhenSupported,
}

#[derive(Debug, Clone, Copy)]
pub struct TransferConfig {
    pub part_size: usize,
    pub concurrency: usize,
}

impl Default for TransferConfig {
    fn default() -> Self {
        Self {
            part_size: 8 * 1024 * 1024,
            concurrency: 4,
        }
    }
}

impl TransferConfig {
    pub(crate) fn validate(&self) -> S3Result<()> {
        if !(MIN_PART_SIZE..=MAX_PART_SIZE).contains(&self.part_size) {
            return Err(S3Error::InvalidConfig(format!(
                "part_size must be between {MIN_PART_SIZE} and {MAX_PART_SIZE} bytes, got {}",
                self.part_size
            )));
        }
        if self.concurrency == 0 {
            return Err(S3Error::InvalidConfig("concurrency must be at least 1".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint_url: Option<String>,
    pub credentials: Option<S3Credentials>,
    pub addressing_style: Option<AddressingStyle>,
    pub checksum: ChecksumMode,
    pub max_attempts: u32,
    pub connect_timeout: Duration,
    pub read_timeout: Option<Duration>,
    pub operation_attempt_timeout: Option<Duration>,
    pub operation_timeout: Option<Duration>,
    pub transfer: TransferConfig,
}

impl S3Config {
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            region: "us-east-1".into(),
            endpoint_url: None,
            credentials: None,
            addressing_style: None,
            checksum: ChecksumMode::WhenRequired,
            max_attempts: 5,
            connect_timeout: Duration::from_secs(5),
            read_timeout: Some(Duration::from_secs(30)),
            operation_attempt_timeout: None,
            operation_timeout: None,
            transfer: TransferConfig::default(),
        }
    }

    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = region.into();
        self
    }

    pub fn endpoint(mut self, endpoint_url: impl Into<String>) -> Self {
        self.endpoint_url = Some(endpoint_url.into());
        self
    }

    pub fn static_credentials(mut self, access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        self.credentials = Some(S3Credentials::Static {
            access_key: access_key.into(),
            secret_key: secret_key.into(),
            session_token: None,
        });
        self
    }

    pub fn credentials(mut self, credentials: S3Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn addressing_style(mut self, style: AddressingStyle) -> Self {
        self.addressing_style = Some(style);
        self
    }

    pub fn checksum(mut self, mode: ChecksumMode) -> Self {
        self.checksum = mode;
        self
    }

    pub fn max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    pub fn read_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.read_timeout = timeout;
        self
    }

    pub fn operation_attempt_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.operation_attempt_timeout = timeout;
        self
    }

    pub fn operation_timeout(mut self, timeout: Option<Duration>) -> Self {
        self.operation_timeout = timeout;
        self
    }

    pub fn transfer(mut self, transfer: TransferConfig) -> Self {
        self.transfer = transfer;
        self
    }

    pub(crate) fn force_path_style(&self) -> bool {
        match self.addressing_style {
            Some(style) => style == AddressingStyle::Path,
            None => self.endpoint_url.is_some(),
        }
    }

    pub(crate) fn validate(&self) -> S3Result<()> {
        if self.bucket.is_empty() {
            return Err(S3Error::InvalidConfig("bucket must not be empty".into()));
        }
        if self.max_attempts == 0 {
            return Err(S3Error::InvalidConfig("max_attempts must be at least 1".into()));
        }
        self.transfer.validate()
    }
}
