use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::Subscription;

#[derive(Debug, Deserialize)]
pub struct CreateChannel {
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannel {
    pub title: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(rename = "currentPage", default = "default_page")]
    pub current_page: i64,
    #[serde(rename = "pageSize", default = "default_page_size")]
    pub page_size: i64,
}

fn default_page() -> i64 {
    1
}

fn default_page_size() -> i64 {
    10
}

impl PaginationParams {
    pub fn limit(&self) -> i64 {
        self.page_size.clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        self.limit() * (self.current_page.max(1) - 1)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelRead {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub items: Vec<T>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

impl From<Subscription> for ChannelRead {
    fn from(s: Subscription) -> Self {
        Self {
            id: s.id,
            title: s.title,
            description: s.description,
            avatar_url: s.avatar_url,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
    #[serde(rename = "currentPage", default = "default_page")]
    pub current_page: i64,
    #[serde(rename = "pageSize", default = "default_page_size")]
    pub page_size: i64,
}

impl SearchParams {
    pub fn limit(&self) -> i64 {
        self.page_size.clamp(1, 100)
    }

    pub fn offset(&self) -> i64 {
        self.limit() * (self.current_page.max(1) - 1)
    }
}

pub type SubscriptionPage = PaginatedResponse<Subscription>;
pub type SubscriberPage = PaginatedResponse<Uuid>;
pub type ChannelReadPage = PaginatedResponse<ChannelRead>;
