use std::{fmt, sync::Arc, time::Duration};

use rdkafka::{ClientConfig, config::RDKafkaLogLevel, statistics::Statistics};

use crate::error::{KafkaError, KafkaResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityProtocol {
    Plaintext,
    Ssl,
    SaslPlaintext,
    SaslSsl,
}

impl SecurityProtocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plaintext => "plaintext",
            Self::Ssl => "ssl",
            Self::SaslPlaintext => "sasl_plaintext",
            Self::SaslSsl => "sasl_ssl",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaslMechanism {
    Plain,
    ScramSha256,
    ScramSha512,
    Gssapi,
}

impl SaslMechanism {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "PLAIN",
            Self::ScramSha256 => "SCRAM-SHA-256",
            Self::ScramSha512 => "SCRAM-SHA-512",
            Self::Gssapi => "GSSAPI",
        }
    }
}

#[derive(Clone)]
pub struct SaslConfig {
    pub mechanism: SaslMechanism,
    pub username: String,
    pub password: String,
}

impl fmt::Debug for SaslConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaslConfig")
            .field("mechanism", &self.mechanism)
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

#[derive(Clone, Default)]
pub struct TlsConfig {
    pub ca_location: Option<String>,
    pub certificate_location: Option<String>,
    pub key_location: Option<String>,
    pub key_password: Option<String>,
    pub disable_hostname_verification: bool,
}

impl fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
            .field("ca_location", &self.ca_location)
            .field("certificate_location", &self.certificate_location)
            .field("key_location", &self.key_location)
            .field("key_password", &self.key_password.as_ref().map(|_| "<redacted>"))
            .field("disable_hostname_verification", &self.disable_hostname_verification)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warning,
    Info,
    Debug,
}

impl From<LogLevel> for RDKafkaLogLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => RDKafkaLogLevel::Error,
            LogLevel::Warning => RDKafkaLogLevel::Warning,
            LogLevel::Info => RDKafkaLogLevel::Info,
            LogLevel::Debug => RDKafkaLogLevel::Debug,
        }
    }
}

#[derive(Clone)]
pub struct StatisticsCallback(pub Arc<dyn Fn(&Statistics) + Send + Sync>);

impl fmt::Debug for StatisticsCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StatisticsCallback")
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub brokers: String,
    pub client_id: Option<String>,
    pub security_protocol: SecurityProtocol,
    pub sasl: Option<SaslConfig>,
    pub tls: Option<TlsConfig>,
    pub log_level: LogLevel,
    pub statistics_interval: Option<Duration>,
    pub on_statistics: Option<StatisticsCallback>,
    pub extra: Vec<(String, String)>,
}

impl ConnectionConfig {
    pub fn new(brokers: impl Into<String>) -> Self {
        Self {
            brokers: brokers.into(),
            client_id: None,
            security_protocol: SecurityProtocol::Plaintext,
            sasl: None,
            tls: None,
            log_level: LogLevel::Warning,
            statistics_interval: None,
            on_statistics: None,
            extra: Vec::new(),
        }
    }

    pub(crate) fn validate(&self) -> KafkaResult<()> {
        if self.brokers.trim().is_empty() {
            return Err(KafkaError::InvalidConfig("brokers must not be empty".into()));
        }
        let uses_sasl = matches!(
            self.security_protocol,
            SecurityProtocol::SaslPlaintext | SecurityProtocol::SaslSsl
        );
        if uses_sasl && self.sasl.is_none() {
            return Err(KafkaError::InvalidConfig("SASL protocol requires `sasl` settings".into()));
        }
        Ok(())
    }

    pub(crate) fn apply(&self, cfg: &mut ClientConfig) {
        cfg.set("bootstrap.servers", &self.brokers)
            .set("security.protocol", self.security_protocol.as_str())
            .set_log_level(self.log_level.into());

        if let Some(client_id) = &self.client_id {
            cfg.set("client.id", client_id);
        }
        if let Some(sasl) = &self.sasl {
            cfg.set("sasl.mechanism", sasl.mechanism.as_str())
                .set("sasl.username", &sasl.username)
                .set("sasl.password", &sasl.password);
        }
        if let Some(tls) = &self.tls {
            let pairs = [
                ("ssl.ca.location", &tls.ca_location),
                ("ssl.certificate.location", &tls.certificate_location),
                ("ssl.key.location", &tls.key_location),
                ("ssl.key.password", &tls.key_password),
            ];
            for (key, value) in pairs {
                if let Some(value) = value {
                    cfg.set(key, value);
                }
            }
            if tls.disable_hostname_verification {
                cfg.set("ssl.endpoint.identification.algorithm", "none");
            }
        }
        if let Some(interval) = self.statistics_interval {
            cfg.set("statistics.interval.ms", interval.as_millis().to_string());
        }
        for (key, value) in &self.extra {
            cfg.set(key, value);
        }
    }
}

macro_rules! connection_setters {
    () => {
        pub fn client_id(mut self, client_id: impl Into<String>) -> Self {
            self.connection.client_id = Some(client_id.into());
            self
        }

        pub fn security_protocol(mut self, protocol: $crate::config::SecurityProtocol) -> Self {
            self.connection.security_protocol = protocol;
            self
        }

        pub fn sasl(
            mut self,
            mechanism: $crate::config::SaslMechanism,
            username: impl Into<String>,
            password: impl Into<String>,
        ) -> Self {
            self.connection.sasl = Some($crate::config::SaslConfig {
                mechanism,
                username: username.into(),
                password: password.into(),
            });
            self
        }

        pub fn tls(mut self, tls: $crate::config::TlsConfig) -> Self {
            self.connection.tls = Some(tls);
            self
        }

        pub fn log_level(mut self, level: $crate::config::LogLevel) -> Self {
            self.connection.log_level = level;
            self
        }

        pub fn statistics(
            mut self,
            interval: std::time::Duration,
            callback: impl Fn(&rdkafka::statistics::Statistics) + Send + Sync + 'static,
        ) -> Self {
            self.connection.statistics_interval = Some(interval);
            self.connection.on_statistics = Some($crate::config::StatisticsCallback(std::sync::Arc::new(callback)));
            self
        }

        pub fn set(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            self.connection.extra.push((key.into(), value.into()));
            self
        }
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Acks {
    All,
    Leader,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compression {
    None,
    Gzip,
    Snappy,
    Lz4,
    Zstd,
}

impl Compression {
    fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
            Self::Snappy => "snappy",
            Self::Lz4 => "lz4",
            Self::Zstd => "zstd",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProducerConfig {
    pub connection: ConnectionConfig,
    pub default_topic: Option<String>,
    pub delivery_timeout: Duration,
    pub queue_timeout: Duration,
    pub acks: Acks,
    pub idempotence: bool,
    pub compression: Compression,
    pub linger: Duration,
}

impl ProducerConfig {
    pub fn new(brokers: impl Into<String>) -> Self {
        Self {
            connection: ConnectionConfig::new(brokers),
            default_topic: None,
            delivery_timeout: Duration::from_secs(30),
            queue_timeout: Duration::from_secs(5),
            acks: Acks::All,
            idempotence: true,
            compression: Compression::Lz4,
            linger: Duration::from_millis(5),
        }
    }

    connection_setters!();

    pub fn default_topic(mut self, topic: impl Into<String>) -> Self {
        self.default_topic = Some(topic.into());
        self
    }

    pub fn delivery_timeout(mut self, timeout: Duration) -> Self {
        self.delivery_timeout = timeout;
        self
    }

    pub fn queue_timeout(mut self, timeout: Duration) -> Self {
        self.queue_timeout = timeout;
        self
    }

    pub fn acks(mut self, acks: Acks) -> Self {
        self.acks = acks;
        self
    }

    pub fn idempotence(mut self, enabled: bool) -> Self {
        self.idempotence = enabled;
        self
    }

    pub fn compression(mut self, compression: Compression) -> Self {
        self.compression = compression;
        self
    }

    pub fn linger(mut self, linger: Duration) -> Self {
        self.linger = linger;
        self
    }

    pub(crate) fn validate(&self) -> KafkaResult<()> {
        self.connection.validate()?;
        if self.idempotence && self.acks != Acks::All {
            return Err(KafkaError::InvalidConfig("idempotence requires `Acks::All`".into()));
        }
        if self.default_topic.as_deref().is_some_and(str::is_empty) {
            return Err(KafkaError::InvalidConfig("default_topic must not be empty".into()));
        }
        Ok(())
    }

    pub(crate) fn to_client_config(&self) -> ClientConfig {
        let mut cfg = ClientConfig::new();
        cfg.set("message.timeout.ms", self.delivery_timeout.as_millis().to_string())
            .set(
                "acks",
                match self.acks {
                    Acks::All => "all",
                    Acks::Leader => "1",
                    Acks::None => "0",
                },
            )
            .set("enable.idempotence", self.idempotence.to_string())
            .set("compression.type", self.compression.as_str())
            .set("linger.ms", self.linger.as_millis().to_string());
        self.connection.apply(&mut cfg);
        cfg
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetReset {
    Earliest,
    Latest,
}

impl OffsetReset {
    fn as_str(self) -> &'static str {
        match self {
            Self::Earliest => "earliest",
            Self::Latest => "latest",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumerMode {
    Group { group_id: String },
    Broadcast { group_prefix: String },
}

#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    pub connection: ConnectionConfig,
    pub mode: ConsumerMode,
    pub topics: Vec<String>,
    pub auto_offset_reset: OffsetReset,
    pub session_timeout: Duration,
    pub commit_interval: Duration,
    pub max_poll_interval: Duration,
}

impl ConsumerConfig {
    pub fn group<I, S>(brokers: impl Into<String>, group_id: impl Into<String>, topics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_mode(
            brokers,
            ConsumerMode::Group {
                group_id: group_id.into(),
            },
            topics,
        )
    }

    pub fn broadcast<I, S>(brokers: impl Into<String>, group_prefix: impl Into<String>, topics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::with_mode(
            brokers,
            ConsumerMode::Broadcast {
                group_prefix: group_prefix.into(),
            },
            topics,
        )
    }

    fn with_mode<I, S>(brokers: impl Into<String>, mode: ConsumerMode, topics: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            connection: ConnectionConfig::new(brokers),
            mode,
            topics: topics.into_iter().map(Into::into).collect(),
            auto_offset_reset: OffsetReset::Earliest,
            session_timeout: Duration::from_secs(45),
            commit_interval: Duration::from_secs(5),
            max_poll_interval: Duration::from_secs(300),
        }
    }

    connection_setters!();

    pub fn auto_offset_reset(mut self, reset: OffsetReset) -> Self {
        self.auto_offset_reset = reset;
        self
    }

    pub fn session_timeout(mut self, timeout: Duration) -> Self {
        self.session_timeout = timeout;
        self
    }

    pub fn commit_interval(mut self, interval: Duration) -> Self {
        self.commit_interval = interval;
        self
    }

    pub fn max_poll_interval(mut self, interval: Duration) -> Self {
        self.max_poll_interval = interval;
        self
    }

    pub(crate) fn commits_enabled(&self) -> bool {
        matches!(self.mode, ConsumerMode::Group { .. })
    }

    pub(crate) fn validate(&self) -> KafkaResult<()> {
        self.connection.validate()?;
        if self.topics.is_empty() || self.topics.iter().any(String::is_empty) {
            return Err(KafkaError::InvalidConfig("topics must be non-empty".into()));
        }
        match &self.mode {
            ConsumerMode::Group { group_id } if group_id.is_empty() => {
                Err(KafkaError::InvalidConfig("group_id must not be empty".into()))
            }
            ConsumerMode::Broadcast { group_prefix } if group_prefix.is_empty() => {
                Err(KafkaError::InvalidConfig("group_prefix must not be empty".into()))
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn group_id(&self) -> String {
        match &self.mode {
            ConsumerMode::Group { group_id } => group_id.clone(),
            ConsumerMode::Broadcast { group_prefix } => format!("{group_prefix}-{}", instance_id()),
        }
    }

    pub(crate) fn to_client_config(&self, group_id: &str) -> ClientConfig {
        let (commit, offset_reset) = match self.mode {
            ConsumerMode::Group { .. } => (true, self.auto_offset_reset),
            ConsumerMode::Broadcast { .. } => (false, OffsetReset::Latest),
        };

        let mut cfg = ClientConfig::new();
        cfg.set("group.id", group_id)
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", self.session_timeout.as_millis().to_string())
            .set("max.poll.interval.ms", self.max_poll_interval.as_millis().to_string())
            .set("enable.auto.offset.store", "false")
            .set("enable.auto.commit", commit.to_string())
            .set("auto.commit.interval.ms", self.commit_interval.as_millis().to_string())
            .set("auto.offset.reset", offset_reset.as_str());
        self.connection.apply(&mut cfg);
        cfg
    }
}

fn instance_id() -> String {
    let host = std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|h| h.trim().to_owned())
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "unknown".into());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    format!("{host}-{}-{nanos:x}", std::process::id())
}
