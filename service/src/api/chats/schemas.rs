use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Join { username: String },
    Chat { message: String },
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct MessagePayload {
    pub id: String,
    pub username: String,
    pub text: String,
    pub ts: u64,
}
