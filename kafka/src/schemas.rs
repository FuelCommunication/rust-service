use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct KafkaMessage<T = String> {
    pub user_id: String,
    pub action: Action,
    pub data: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Action {
    Create,
    Update,
    Delete,
}
