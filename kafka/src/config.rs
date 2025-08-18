use rdkafka::config::RDKafkaLogLevel;

pub struct ConsumerConfig {
    pub brokers: String,
    pub group_id: String,
    pub input_topic: String,
    pub log_level: RDKafkaLogLevel,
}

pub struct ProducerConfig {
    pub brokers: String,
    pub topic: String,
}

impl ConsumerConfig {
    pub fn new(
        brokers: impl Into<String>,
        group_id: impl Into<String>,
        input_topic: impl Into<String>,
        log_level: u8,
    ) -> Self {
        let log_level = match log_level {
            0 => RDKafkaLogLevel::Emerg,
            1 => RDKafkaLogLevel::Alert,
            2 => RDKafkaLogLevel::Critical,
            3 => RDKafkaLogLevel::Error,
            4 => RDKafkaLogLevel::Warning,
            5 => RDKafkaLogLevel::Notice,
            6 => RDKafkaLogLevel::Info,
            _ => RDKafkaLogLevel::Debug,
        };

        Self {
            brokers: brokers.into(),
            group_id: group_id.into(),
            input_topic: input_topic.into(),
            log_level,
        }
    }
}

impl ProducerConfig {
    pub fn new(brokers: impl Into<String>, topic: impl Into<String>) -> Self {
        Self {
            brokers: brokers.into(),
            topic: topic.into(),
        }
    }
}
