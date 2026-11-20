use kafka_client::{
    ConsumerConfig, KafkaConsumer, KafkaError, KafkaProducer, ProducerConfig,
    config::{Acks, SecurityProtocol},
};

fn is_invalid_config<T>(result: Result<T, KafkaError>) -> bool {
    matches!(result, Err(KafkaError::InvalidConfig(_)))
}

#[test]
fn producer_valid_config() {
    let config = ProducerConfig::new("localhost:9092")
        .default_topic("events")
        .client_id("test")
        .set("socket.keepalive.enable", "true");
    assert!(KafkaProducer::new(config).is_ok());
}

#[test]
fn producer_rejects_empty_brokers() {
    assert!(is_invalid_config(KafkaProducer::new(ProducerConfig::new(""))));
}

#[test]
fn producer_rejects_idempotence_without_acks_all() {
    let config = ProducerConfig::new("localhost:9092").acks(Acks::Leader);
    assert!(is_invalid_config(KafkaProducer::new(config)));
    let config = ProducerConfig::new("localhost:9092").acks(Acks::Leader).idempotence(false);
    assert!(KafkaProducer::new(config).is_ok());
}

#[test]
fn sasl_protocol_requires_credentials() {
    let config = ProducerConfig::new("localhost:9092").security_protocol(SecurityProtocol::SaslPlaintext);
    assert!(is_invalid_config(KafkaProducer::new(config)));
}

#[test]
fn unknown_librdkafka_property_is_rejected() {
    let config = ProducerConfig::new("localhost:9092").set("no.such.property", "1");
    assert!(matches!(KafkaProducer::new(config), Err(KafkaError::Kafka(_))));
}

#[test]
fn sasl_debug_redacts_password() {
    let config = ProducerConfig::new("localhost:9092").sasl(kafka_client::config::SaslMechanism::Plain, "user", "super-secret");
    assert!(!format!("{config:?}").contains("super-secret"));
}

#[tokio::test]
async fn consumer_rejects_empty_topics_and_group() {
    let no_topics: [&str; 0] = [];
    assert!(is_invalid_config(KafkaConsumer::new(ConsumerConfig::group(
        "localhost:9092",
        "g",
        no_topics
    ))));
    assert!(is_invalid_config(KafkaConsumer::new(ConsumerConfig::group(
        "localhost:9092",
        "",
        ["t"]
    ))));
}

#[tokio::test]
async fn broadcast_generates_unique_group_ids() -> anyhow::Result<()> {
    let a = KafkaConsumer::new(ConsumerConfig::broadcast("localhost:9092", "chats", ["t"]))?;
    let b = KafkaConsumer::new(ConsumerConfig::broadcast("localhost:9092", "chats", ["t"]))?;
    assert!(a.group_id().starts_with("chats-"));
    assert_ne!(a.group_id(), b.group_id());
    Ok(())
}
