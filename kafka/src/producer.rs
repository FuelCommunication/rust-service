use crate::{config::ProducerConfig, error::KafkaResult, schemas::KafkaMessage};
use rdkafka::{
    ClientConfig,
    producer::{FutureProducer, FutureRecord},
};

pub struct KafkaProducer {
    producer: FutureProducer,
    topic: String,
}

impl KafkaProducer {
    pub fn new(config: ProducerConfig) -> KafkaResult<Self> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", config.brokers)
            .set("message.timeout.ms", "5000")
            .set("allow.auto.create.topics", "true")
            .create::<FutureProducer>()?;
        Ok(Self {
            producer,
            topic: config.topic,
        })
    }

    pub async fn send(&self, message: &KafkaMessage) -> KafkaResult<()> {
        let payload = serde_json::to_vec(message)?;
        let key = &message.user_id;

        let record = FutureRecord::to(&self.topic).payload(&payload).key(key);

        self.producer.send_result(record)?.await??;
        Ok(())
    }
}
