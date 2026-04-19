use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Join { username: String },
    Chat(MessagePayload),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessagePayload {
    pub user_id: Uuid,
    pub username: String,
    pub text: String,
    pub ts: u64,
}

pub struct Room {
    pub sender: broadcast::Sender<MessagePayload>,
}
