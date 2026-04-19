use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Channel {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ChannelWithTotal {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub total: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ChannelSubscriber {
    pub id: Uuid,
    pub user_id: Uuid,
    pub channel_id: Uuid,
    pub is_owner: bool,
}

#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct Subscription {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub subscribers_count: i64,
}
