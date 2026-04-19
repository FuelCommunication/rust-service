use kafka::{
    config::{ConsumerConfig, LogLevel, ProducerConfig},
    consumer::KafkaConsumer,
    producer::KafkaProducer,
    schemas::{Action, KafkaMessage},
};
use testcontainers_modules::{kafka::Kafka, testcontainers::runners::AsyncRunner as _};

#[tokio::test]
async fn test_producer_consumer_integration() -> anyhow::Result<()> {
    let kafka = Kafka::default().start().await?;
    let host = kafka.get_host().await?;
    let port = kafka.get_host_port_ipv4(9093).await?;
    let brokers = format!("{}:{}", host, port);

    let producer_config = ProducerConfig::new(brokers.clone(), "integration-test")?;
    let producer = KafkaProducer::new(producer_config)?;

    let consumer_config = ConsumerConfig::new(brokers, "test-group", "integration-test", LogLevel::Info)?;
    let consumer = KafkaConsumer::new(consumer_config)?;

    let test_message = KafkaMessage {
        user_id: "integration_user".to_string(),
        action: Action::Create,
        data: Some("integration data".to_string()),
    };

    producer.send(&test_message).await?;
    let received = consumer.consume().await?;

    assert_eq!(received.user_id, "integration_user");
    assert_eq!(received.action, Action::Create);
    assert_eq!(received.data, Some("integration data".to_string()));

    Ok(())
}
