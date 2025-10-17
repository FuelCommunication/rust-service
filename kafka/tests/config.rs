use kafka::{
    config::{ConsumerConfig, LogLevel, ProducerConfig},
    error::KafkaResult,
};
use rdkafka::config::RDKafkaLogLevel;

#[test]
fn test_consumer_config_creation() -> KafkaResult<()> {
    let config = ConsumerConfig::new("localhost:9092", "test-group", "test-topic", LogLevel::Info)?;

    assert_eq!(config.brokers, "localhost:9092");
    assert_eq!(config.group_id, "test-group");
    assert_eq!(config.input_topic, "test-topic");
    Ok(())
}

#[test]
fn test_producer_config_creation() -> KafkaResult<()> {
    let config = ProducerConfig::new("localhost:9092", "output-topic")?;

    assert_eq!(config.brokers, "localhost:9092");
    assert_eq!(config.topic, "output-topic");
    Ok(())
}

#[test]
fn test_log_level_conversion() {
    let level = LogLevel::Debug;
    let rdkafka_level = level.into();
    assert!(matches!(rdkafka_level, RDKafkaLogLevel::Debug));
}

#[test]
fn test_consumer_validate() {
    let config = ConsumerConfig::new("", "", "test-topic", LogLevel::Info);
    assert!(config.is_err());
}

#[test]
fn test_producer_validate() {
    let config = ProducerConfig::new("", "");
    assert!(config.is_err());
}
