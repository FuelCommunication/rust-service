use meilisearch_sdk::{client::Client, indexes::Index};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ApiError;

const INDEX_NAME: &str = "channels";

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelDocument {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
}

pub struct SearchService {
    index: Index,
}

impl SearchService {
    pub async fn new(url: &str, api_key: Option<&str>) -> Result<Self, ApiError> {
        let client = Client::new(url, api_key).expect("Failed to create Meilisearch client");
        let index = client.index(INDEX_NAME);

        let task = index
            .set_searchable_attributes(["title", "description"])
            .await
            .map_err(|e| ApiError::Internal(format!("Meilisearch config error: {e}")))?;
        task.wait_for_completion(&client, None, None)
            .await
            .map_err(|e| ApiError::Internal(format!("Meilisearch task error: {e}")))?;

        tracing::info!("Meilisearch index '{INDEX_NAME}' configured");
        Ok(Self { index })
    }

    pub async fn index_channel(&self, id: Uuid, title: &str, description: Option<&str>, avatar_url: Option<&str>) {
        let doc = ChannelDocument {
            id: id.to_string(),
            title: title.to_string(),
            description: description.map(String::from),
            avatar_url: avatar_url.map(String::from),
        };

        if let Err(e) = self.index.add_documents(&[doc], Some("id")).await {
            tracing::error!("Failed to index channel {id}: {e}");
        }
    }

    pub async fn update_channel(&self, id: Uuid, title: &str, description: Option<&str>, avatar_url: Option<&str>) {
        self.index_channel(id, title, description, avatar_url).await;
    }

    pub async fn delete_channel(&self, id: Uuid) {
        if let Err(e) = self.index.delete_document(&id.to_string()).await {
            tracing::error!("Failed to delete channel {id} from search index: {e}");
        }
    }

    pub async fn search(&self, query: &str, limit: i64, offset: i64) -> Result<(Vec<ChannelDocument>, i64), ApiError> {
        let results = self
            .index
            .search()
            .with_query(query)
            .with_limit(limit as usize)
            .with_offset(offset as usize)
            .execute::<ChannelDocument>()
            .await
            .map_err(|e| ApiError::Internal(format!("Meilisearch search error: {e}")))?;

        let total = results.estimated_total_hits.unwrap_or(results.hits.len()) as i64;
        let items = results.hits.into_iter().map(|h| h.result).collect();

        Ok((items, total))
    }
}
