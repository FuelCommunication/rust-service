use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientEvent {
    Chat { text: String },
    Edit { message_id: Uuid, text: String },
    Delete { message_id: Uuid },
    Typing,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    Message(MessagePayload),
    Edited { message_id: Uuid, text: String, ts: u64 },
    Deleted { message_id: Uuid },
    Typing { user_id: Uuid, username: String },
    History { messages: Vec<MessagePayload> },
    Error { text: String },
}

#[derive(Debug, Serialize, Clone)]
pub struct MessagePayload {
    pub message_id: Uuid,
    pub user_id: Uuid,
    pub username: String,
    pub text: String,
    pub ts: u64,
}
