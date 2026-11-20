use std::time::Duration;

use rdkafka::{
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    types::RDKafkaErrorCode,
};

use crate::{
    config::ConnectionConfig,
    context::TracingContext,
    error::{KafkaError, KafkaResult},
};

#[derive(Debug, Clone)]
pub struct TopicSpec {
    pub name: String,
    pub partitions: i32,
    pub replication_factor: i32,
    pub config: Vec<(String, String)>,
}

impl TopicSpec {
    pub fn new(name: impl Into<String>, partitions: i32, replication_factor: i32) -> Self {
        Self {
            name: name.into(),
            partitions,
            replication_factor,
            config: Vec::new(),
        }
    }
}

pub struct KafkaAdmin {
    client: AdminClient<TracingContext>,
    timeout: Duration,
}

impl KafkaAdmin {
    pub fn new(connection: &ConnectionConfig) -> KafkaResult<Self> {
        connection.validate()?;
        let mut cfg = rdkafka::ClientConfig::new();
        connection.apply(&mut cfg);
        let client = cfg.create_with_context(TracingContext {
            on_statistics: connection.on_statistics.clone(),
            commit_on_revoke: false,
        })?;
        Ok(Self {
            client,
            timeout: Duration::from_secs(30),
        })
    }

    pub async fn ensure_topics(&self, topics: &[TopicSpec]) -> KafkaResult<()> {
        let new_topics: Vec<NewTopic<'_>> = topics
            .iter()
            .map(|t| {
                t.config.iter().fold(
                    NewTopic::new(&t.name, t.partitions, TopicReplication::Fixed(t.replication_factor)),
                    |nt, (k, v)| nt.set(k, v),
                )
            })
            .collect();
        let opts = AdminOptions::new().operation_timeout(Some(self.timeout));

        for result in self.client.create_topics(&new_topics, &opts).await? {
            match result {
                Ok(name) => tracing::info!(topic = %name, "Created topic"),
                Err((_, RDKafkaErrorCode::TopicAlreadyExists)) => {}
                Err((name, code)) => {
                    return Err(KafkaError::Kafka(rdkafka::error::KafkaError::AdminOp(code)))
                        .inspect_err(|_| tracing::error!(topic = %name, ?code, "Failed to create topic"));
                }
            }
        }
        Ok(())
    }

    pub async fn delete_topics(&self, names: &[&str]) -> KafkaResult<()> {
        let opts = AdminOptions::new().operation_timeout(Some(self.timeout));
        for result in self.client.delete_topics(names, &opts).await? {
            match result {
                Ok(_) | Err((_, RDKafkaErrorCode::UnknownTopicOrPartition)) => {}
                Err((_, code)) => return Err(KafkaError::Kafka(rdkafka::error::KafkaError::AdminOp(code))),
            }
        }
        Ok(())
    }
}
