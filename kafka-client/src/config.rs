use crate::error::{KafkaError, KafkaResult};
use rdkafka::config::RDKafkaLogLevel;

#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    pub brokers: String,
    pub group_id: String,
    pub input_topic: String,
    pub log_level: RDKafkaLogLevel,
    pub session_timeout_ms: u32,
    pub auto_commit: bool,
    pub auto_commit_interval_ms: u32,
    pub auto_offset_reset: OffsetReset,
}

#[derive(Debug, Clone)]
pub struct ProducerConfig {
    pub brokers: String,
    pub topic: String,
    pub message_timeout_ms: u32,
    pub retries: u32,
    pub auto_create_topics: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum OffsetReset {
    Earliest,
    Latest,
}

impl OffsetReset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Earliest => "earliest",
            Self::Latest => "latest",
        }
    }
}

pub struct ConsumerConfigBuilder {
    brokers: String,
    group_id: String,
    input_topic: String,
    log_level: RDKafkaLogLevel,
    session_timeout_ms: u32,
    auto_commit: bool,
    auto_commit_interval_ms: u32,
    auto_offset_reset: OffsetReset,
}

impl ConsumerConfigBuilder {
    pub fn session_timeout_ms(mut self, ms: u32) -> Self {
        self.session_timeout_ms = ms;
        self
    }

    pub fn auto_commit(mut self, enabled: bool) -> Self {
        self.auto_commit = enabled;
        self
    }

    pub fn auto_commit_interval_ms(mut self, ms: u32) -> Self {
        self.auto_commit_interval_ms = ms;
        self
    }

    pub fn auto_offset_reset(mut self, reset: OffsetReset) -> Self {
        self.auto_offset_reset = reset;
        self
    }

    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = level.into();
        self
    }

    pub fn build(self) -> KafkaResult<ConsumerConfig> {
        if self.brokers.is_empty() {
            return Err(KafkaError::InvalidConfig("Brokers cannot be empty".into()));
        }
        if self.group_id.is_empty() {
            return Err(KafkaError::InvalidConfig("Group ID cannot be empty".into()));
        }

        Ok(ConsumerConfig {
            brokers: self.brokers,
            group_id: self.group_id,
            input_topic: self.input_topic,
            log_level: self.log_level,
            session_timeout_ms: self.session_timeout_ms,
            auto_commit: self.auto_commit,
            auto_commit_interval_ms: self.auto_commit_interval_ms,
            auto_offset_reset: self.auto_offset_reset,
        })
    }
}

impl ConsumerConfig {
    pub fn builder(
        brokers: impl Into<String>,
        group_id: impl Into<String>,
        input_topic: impl Into<String>,
    ) -> ConsumerConfigBuilder {
        ConsumerConfigBuilder {
            brokers: brokers.into(),
            group_id: group_id.into(),
            input_topic: input_topic.into(),
            log_level: RDKafkaLogLevel::Info,
            session_timeout_ms: 6000,
            auto_commit: true,
            auto_commit_interval_ms: 5000,
            auto_offset_reset: OffsetReset::Earliest,
        }
    }
}

pub struct ProducerConfigBuilder {
    brokers: String,
    topic: String,
    message_timeout_ms: u32,
    retries: u32,
    auto_create_topics: bool,
}

impl ProducerConfigBuilder {
    pub fn message_timeout_ms(mut self, ms: u32) -> Self {
        self.message_timeout_ms = ms;
        self
    }

    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    pub fn auto_create_topics(mut self, enabled: bool) -> Self {
        self.auto_create_topics = enabled;
        self
    }

    pub fn build(self) -> KafkaResult<ProducerConfig> {
        if self.brokers.is_empty() {
            return Err(KafkaError::InvalidConfig("Brokers cannot be empty".into()));
        }
        if self.topic.is_empty() {
            return Err(KafkaError::InvalidConfig("Topic cannot be empty".into()));
        }

        Ok(ProducerConfig {
            brokers: self.brokers,
            topic: self.topic,
            message_timeout_ms: self.message_timeout_ms,
            retries: self.retries,
            auto_create_topics: self.auto_create_topics,
        })
    }
}

impl ProducerConfig {
    pub fn builder(brokers: impl Into<String>, topic: impl Into<String>) -> ProducerConfigBuilder {
        ProducerConfigBuilder {
            brokers: brokers.into(),
            topic: topic.into(),
            message_timeout_ms: 5000,
            retries: 3,
            auto_create_topics: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Emerg = 0,
    Alert = 1,
    Critical = 2,
    Error = 3,
    Warning = 4,
    Notice = 5,
    Info = 6,
    Debug = 7,
}

impl From<LogLevel> for RDKafkaLogLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Emerg => RDKafkaLogLevel::Emerg,
            LogLevel::Alert => RDKafkaLogLevel::Alert,
            LogLevel::Critical => RDKafkaLogLevel::Critical,
            LogLevel::Error => RDKafkaLogLevel::Error,
            LogLevel::Warning => RDKafkaLogLevel::Warning,
            LogLevel::Notice => RDKafkaLogLevel::Notice,
            LogLevel::Info => RDKafkaLogLevel::Info,
            LogLevel::Debug => RDKafkaLogLevel::Debug,
        }
    }
}
