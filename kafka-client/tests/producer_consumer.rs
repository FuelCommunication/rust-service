use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kafka_client::{
    ConnectionConfig, ConsumerConfig, KafkaAdmin, KafkaConsumer, KafkaProducer, ProducerConfig, RetryPolicy, SendOptions,
    TopicSpec,
};
use serde::{Deserialize, Serialize};
use testcontainers_modules::{
    kafka::apache,
    testcontainers::{ContainerAsync, runners::AsyncRunner as _},
};
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Event {
    id: u32,
    name: String,
}

fn event(id: u32) -> Event {
    Event {
        id,
        name: format!("event-{id}"),
    }
}

async fn start_kafka(topics: &[&str]) -> anyhow::Result<(ContainerAsync<apache::Kafka>, String)> {
    let kafka = apache::Kafka::default().start().await?;
    let brokers = format!("127.0.0.1:{}", kafka.get_host_port_ipv4(apache::KAFKA_PORT).await?);

    let admin = KafkaAdmin::new(&ConnectionConfig::new(&brokers))?;
    let specs: Vec<TopicSpec> = topics.iter().map(|t| TopicSpec::new(*t, 1, 1)).collect();
    admin.ensure_topics(&specs).await?;
    admin.ensure_topics(&specs).await?;

    Ok((kafka, brokers))
}

#[tokio::test]
async fn send_and_receive_with_metadata() -> anyhow::Result<()> {
    let (_kafka, brokers) = start_kafka(&["events"]).await?;

    let producer = KafkaProducer::new(ProducerConfig::new(&brokers).default_topic("events"))?;
    let consumer = KafkaConsumer::new(ConsumerConfig::group(&brokers, "meta", ["events"]))?;

    let report = producer
        .send_with(SendOptions::key("user-1").header("trace-id", "abc"), &event(1))
        .await?;
    assert_eq!(report.partition, 0);

    let message = timeout(WAIT, consumer.recv_decoded::<Event>()).await??;
    assert_eq!(message.value, event(1));
    assert_eq!(message.record.key_str(), Some("user-1"));
    assert_eq!(message.record.headers.get_str("trace-id"), Some("abc"));
    assert_eq!(message.record.headers.get_str("content-type"), Some("application/json"));
    assert_eq!(message.record.offset, report.offset);
    assert!(message.record.timestamp.is_some());

    consumer.ack(&message.record)?;
    consumer.close().await;
    producer.shutdown(Duration::from_secs(5)).await?;
    Ok(())
}

#[tokio::test]
async fn unacked_message_is_redelivered() -> anyhow::Result<()> {
    let (_kafka, brokers) = start_kafka(&["redelivery"]).await?;
    let producer = KafkaProducer::new(ProducerConfig::new(&brokers).default_topic("redelivery"))?;
    producer.send("k", &event(1)).await?;
    producer.send("k", &event(2)).await?;

    let config = ConsumerConfig::group(&brokers, "redelivery-group", ["redelivery"]);

    {
        let consumer = KafkaConsumer::new(config.clone())?;
        let first = timeout(WAIT, consumer.recv_decoded::<Event>()).await??;
        assert_eq!(first.value.id, 1);
        consumer.ack(&first.record)?;
        let second = timeout(WAIT, consumer.recv_decoded::<Event>()).await??;
        assert_eq!(second.value.id, 2);
        consumer.close().await;
    }

    let consumer = KafkaConsumer::new(config)?;
    let again = timeout(WAIT, consumer.recv_decoded::<Event>()).await??;
    assert_eq!(again.value.id, 2);
    consumer.close().await;
    Ok(())
}

#[tokio::test]
async fn broadcast_delivers_to_every_instance() -> anyhow::Result<()> {
    let (_kafka, brokers) = start_kafka(&["fanout"]).await?;
    let producer = KafkaProducer::new(ProducerConfig::new(&brokers).default_topic("fanout"))?;
    let a = KafkaConsumer::new(ConsumerConfig::broadcast(&brokers, "chats", ["fanout"]))?;
    let b = KafkaConsumer::new(ConsumerConfig::broadcast(&brokers, "chats", ["fanout"]))?;

    let sender = {
        let producer = producer.clone();
        tokio::spawn(async move {
            for id in 0..u32::MAX {
                let _ = producer.send("k", &event(id)).await;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
        })
    };

    let (ra, rb) = tokio::join!(
        timeout(WAIT, a.recv_decoded::<Event>()),
        timeout(WAIT, b.recv_decoded::<Event>())
    );
    sender.abort();
    ra??;
    rb??;
    Ok(())
}

#[tokio::test]
async fn run_retries_handler_and_skips_poison_messages() -> anyhow::Result<()> {
    let (_kafka, brokers) = start_kafka(&["run"]).await?;

    let raw = KafkaProducer::new(ProducerConfig::new(&brokers).default_topic("run"))?;
    raw.send("k", "not an event").await?;
    raw.send("k", &event(1)).await?;
    raw.send("k", &event(2)).await?;

    let consumer = KafkaConsumer::new(ConsumerConfig::group(&brokers, "run-group", ["run"]))?;
    let handled = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(Mutex::new(0u32));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let stop_tx = Arc::new(Mutex::new(Some(stop_tx)));

    let retry = RetryPolicy {
        max_retries: 2,
        initial_backoff: Duration::from_millis(10),
        max_backoff: Duration::from_millis(50),
    };

    let run = consumer.run::<Event, _, _, String>(
        retry,
        async {
            let _ = stop_rx.await;
        },
        |message| {
            let handled = handled.clone();
            let calls = calls.clone();
            let stop_tx = stop_tx.clone();
            async move {
                let call = {
                    let mut calls = calls.lock().unwrap();
                    *calls += 1;
                    *calls
                };
                if message.value.id == 1 && call == 1 {
                    return Err("transient".to_owned());
                }
                let mut handled = handled.lock().unwrap();
                handled.push(message.value.id);
                if handled.len() == 2
                    && let Some(tx) = stop_tx.lock().unwrap().take()
                {
                    let _ = tx.send(());
                }
                Ok(())
            }
        },
    );
    timeout(WAIT, run).await?;

    assert_eq!(*handled.lock().unwrap(), [1, 2]);
    assert_eq!(*calls.lock().unwrap(), 3);

    let consumer = KafkaConsumer::new(ConsumerConfig::group(&brokers, "run-group", ["run"]))?;
    assert!(timeout(Duration::from_secs(5), consumer.recv()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn send_detached_then_flush() -> anyhow::Result<()> {
    let (_kafka, brokers) = start_kafka(&["detached", "other"]).await?;
    let producer = KafkaProducer::new(ProducerConfig::new(&brokers).default_topic("detached"))?;

    for id in 0..10 {
        producer.send_detached(SendOptions::key("k"), &event(id))?;
    }
    producer.send_detached(SendOptions::key("k").topic("other"), &event(100))?;
    producer.flush(Duration::from_secs(10)).await?;
    assert_eq!(producer.in_flight(), 0);

    let consumer = KafkaConsumer::new(ConsumerConfig::group(&brokers, "detached-group", ["detached", "other"]))?;
    let mut ids = Vec::new();
    for _ in 0..11 {
        ids.push(timeout(WAIT, consumer.recv_decoded::<Event>()).await??.value.id);
    }
    ids.sort_unstable();
    assert_eq!(ids, (0..10).chain([100]).collect::<Vec<_>>());
    Ok(())
}
