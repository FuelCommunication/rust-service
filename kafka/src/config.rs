use crate::error::{KafkaError, KafkaResult};
use rdkafka::config::RDKafkaLogLevel;

#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    pub brokers: String,
    pub group_id: String,
    pub input_topic: String,
    pub log_level: RDKafkaLogLevel,
}

#[derive(Debug, Clone)]
pub struct ProducerConfig {
    pub brokers: String,
    pub topic: String,
}

impl ConsumerConfig {
    pub fn new(
        brokers: impl Into<String>,
        group_id: impl Into<String>,
        input_topic: impl Into<String>,
        log_level: LogLevel,
    ) -> KafkaResult<Self> {
        let brokers = brokers.into();
        let group_id = group_id.into();

        if brokers.is_empty() {
            return Err(KafkaError::InvalidConfig("Brokers cannot be empty".into()));
        }
        if group_id.is_empty() {
            return Err(KafkaError::InvalidConfig("Group ID cannot be empty".into()));
        }

        Ok(Self {
            brokers,
            group_id,
            input_topic: input_topic.into(),
            log_level: log_level.into(),
        })
    }
}

impl ProducerConfig {
    pub fn new(brokers: impl Into<String>, topic: impl Into<String>) -> KafkaResult<Self> {
        let brokers = brokers.into();
        let topic = topic.into();

        if brokers.is_empty() {
            return Err(KafkaError::InvalidConfig("Brokers cannot be empty".into()));
        }
        if topic.is_empty() {
            return Err(KafkaError::InvalidConfig("Group ID cannot be empty".into()));
        }

        Ok(Self { brokers, topic })
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
