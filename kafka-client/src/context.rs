use rdkafka::{
    ClientContext,
    config::RDKafkaLogLevel,
    consumer::{BaseConsumer, CommitMode, Consumer as _, ConsumerContext, Rebalance},
    error::KafkaResult,
    statistics::Statistics,
    topic_partition_list::TopicPartitionList,
};

use crate::config::StatisticsCallback;

#[derive(Debug, Clone, Default)]
pub struct TracingContext {
    pub(crate) on_statistics: Option<StatisticsCallback>,
    pub(crate) commit_on_revoke: bool,
}

impl ClientContext for TracingContext {
    fn log(&self, level: RDKafkaLogLevel, fac: &str, message: &str) {
        match level {
            RDKafkaLogLevel::Emerg | RDKafkaLogLevel::Alert | RDKafkaLogLevel::Critical | RDKafkaLogLevel::Error => {
                tracing::error!(target: "librdkafka", facility = fac, "{message}");
            }
            RDKafkaLogLevel::Warning => tracing::warn!(target: "librdkafka", facility = fac, "{message}"),
            RDKafkaLogLevel::Notice | RDKafkaLogLevel::Info => {
                tracing::info!(target: "librdkafka", facility = fac, "{message}");
            }
            RDKafkaLogLevel::Debug => tracing::debug!(target: "librdkafka", facility = fac, "{message}"),
        }
    }

    fn error(&self, error: rdkafka::error::KafkaError, reason: &str) {
        tracing::error!(target: "librdkafka", error = %error, "{reason}");
    }

    fn stats(&self, statistics: Statistics) {
        if let Some(callback) = &self.on_statistics {
            (callback.0)(&statistics);
        }
    }
}

impl ConsumerContext for TracingContext {
    fn pre_rebalance(&self, consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Revoke(tpl) => {
                tracing::info!(partitions = %format_tpl(tpl), "Partitions revoked");
                if self.commit_on_revoke
                    && let Err(e) = consumer.commit_consumer_state(CommitMode::Sync)
                    && !matches!(e.rdkafka_error_code(), Some(rdkafka::types::RDKafkaErrorCode::NoOffset))
                {
                    tracing::warn!(error = %e, "Failed to commit offsets before revoke");
                }
            }
            Rebalance::Assign(_) => {}
            Rebalance::Error(e) => tracing::error!(error = %e, "Rebalance error"),
        }
    }

    fn post_rebalance(&self, _consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        if let Rebalance::Assign(tpl) = rebalance {
            tracing::info!(partitions = %format_tpl(tpl), "Partitions assigned");
        }
    }

    fn commit_callback(&self, result: KafkaResult<()>, _offsets: &TopicPartitionList) {
        match result {
            Ok(()) => {}
            Err(e) if matches!(e.rdkafka_error_code(), Some(rdkafka::types::RDKafkaErrorCode::NoOffset)) => {}
            Err(e) => tracing::warn!(error = %e, "Offset commit failed"),
        }
    }
}

fn format_tpl(tpl: &TopicPartitionList) -> String {
    tpl.elements()
        .iter()
        .map(|e| format!("{}[{}]", e.topic(), e.partition()))
        .collect::<Vec<_>>()
        .join(", ")
}
