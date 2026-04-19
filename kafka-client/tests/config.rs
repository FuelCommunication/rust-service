use kafka_client::{
    config::{ConsumerConfig, LogLevel, ProducerConfig},
    error::KafkaResult,
};
use rdkafka::config::RDKafkaLogLevel;

#[test]
fn test_consumer_config_creation() -> KafkaResult<()> {
    let config = ConsumerConfig::builder("localhost:9092", "test-group", "test-topic")
        .log_level(LogLevel::Info)
        .build()?;

    assert_eq!(config.brokers, "localhost:9092");
    assert_eq!(config.group_id, "test-group");
    assert_eq!(config.input_topic, "test-topic");
    Ok(())
}

#[test]
fn test_consumer_config_custom_timeouts() -> KafkaResult<()> {
    let config = ConsumerConfig::builder("localhost:9092", "test-group", "test-topic")
        .session_timeout_ms(10_000)
        .auto_commit(false)
        .auto_commit_interval_ms(3000)
        .build()?;

    assert_eq!(config.session_timeout_ms, 10_000);
    assert!(!config.auto_commit);
    assert_eq!(config.auto_commit_interval_ms, 3000);
    Ok(())
}

#[test]
fn test_producer_config_creation() -> KafkaResult<()> {
    let config = ProducerConfig::builder("localhost:9092", "output-topic").build()?;

    assert_eq!(config.brokers, "localhost:9092");
    assert_eq!(config.topic, "output-topic");
    assert!(!config.auto_create_topics);
    assert_eq!(config.retries, 3);
    Ok(())
}

#[test]
fn test_producer_config_custom() -> KafkaResult<()> {
    let config = ProducerConfig::builder("localhost:9092", "output-topic")
        .message_timeout_ms(10_000)
        .retries(5)
        .auto_create_topics(true)
        .build()?;

    assert_eq!(config.message_timeout_ms, 10_000);
    assert_eq!(config.retries, 5);
    assert!(config.auto_create_topics);
    Ok(())
}

#[test]
fn test_log_level_conversion() {
    let level = LogLevel::Debug;
    let rdkafka_level: RDKafkaLogLevel = level.into();
    assert!(matches!(rdkafka_level, RDKafkaLogLevel::Debug));
}

#[test]
fn test_consumer_validate() {
    let config = ConsumerConfig::builder("", "", "test-topic").build();
    assert!(config.is_err());
}

#[test]
fn test_producer_validate() {
    let config = ProducerConfig::builder("", "").build();
    assert!(config.is_err());
}
