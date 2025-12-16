pub mod error;

use chrono::{DateTime, Utc};
use error::ScyllaResult;
use scylla::observability::metrics::Metrics;
use scylla::{
    client::{session::Session, session_builder::SessionBuilder},
    statement::prepared::PreparedStatement,
    value::CqlTimestamp,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message_id: Uuid,
    pub chat_id: Uuid,
    pub user_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub is_deleted: bool,
}

pub struct ChatMessageStore {
    session: Arc<Session>,
    insert_stmt: PreparedStatement,
    get_by_id_stmt: PreparedStatement,
    get_by_chat_stmt: PreparedStatement,
    update_content_stmt: PreparedStatement,
    delete_stmt: PreparedStatement,
}

impl ChatMessageStore {
    pub async fn new(uri: impl AsRef<str>) -> ScyllaResult<Self> {
        let additional_nodes = std::env::var("SCYLLA_NODES")
            .unwrap_or_else(|_| "scylla-node-2:9042,scylla-node-3:9042".to_string())
            .split(',')
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();

        let mut builder = SessionBuilder::new()
            .known_node(uri.as_ref())
            .connection_timeout(Duration::from_secs(3))
            .cluster_metadata_refresh_interval(Duration::from_secs(10));

        for node in additional_nodes {
            tracing::info!("Adding node: {}", node);
            builder = builder.known_node(node);
        }

        let session = Arc::new(builder.build().await?);

        session
            .query_unpaged(
                "CREATE KEYSPACE IF NOT EXISTS chat WITH REPLICATION = {'class': 'SimpleStrategy', 'replication_factor': 3}",
                &[],
            )
            .await?;

        session.query_unpaged("USE chat", &[]).await?;
        session
            .query_unpaged(
                "CREATE TABLE IF NOT EXISTS messages (
                chat_id UUID,
                created_at TIMESTAMP,
                message_id UUID,
                user_id UUID,
                content TEXT,
                updated_at TIMESTAMP,
                is_deleted BOOLEAN,
                PRIMARY KEY ((chat_id), created_at, message_id)
            ) WITH CLUSTERING ORDER BY (created_at DESC)",
                &[],
            )
            .await?;

        session
            .query_unpaged("CREATE INDEX IF NOT EXISTS ON messages (message_id)", &[])
            .await?;

        session
            .query_unpaged(
                "CREATE TABLE IF NOT EXISTS user_messages (
                user_id UUID,
                created_at TIMESTAMP,
                message_id UUID,
                chat_id UUID,
                PRIMARY KEY ((user_id), created_at, message_id)
            ) WITH CLUSTERING ORDER BY (created_at DESC)",
                &[],
            )
            .await?;

        let insert_stmt = session
            .prepare(
                "INSERT INTO messages (message_id, chat_id, user_id, content, created_at, updated_at, is_deleted)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .await?;

        let get_by_id_stmt = session
            .prepare(
                "SELECT message_id, chat_id, user_id, content, created_at, updated_at, is_deleted
         FROM messages WHERE message_id = ?",
            )
            .await?;

        let get_by_chat_stmt = session
            .prepare(
                "SELECT message_id, chat_id, user_id, content, created_at, updated_at, is_deleted
             FROM messages WHERE chat_id = ? LIMIT ?",
            )
            .await?;

        let update_content_stmt = session
            .prepare(
                "UPDATE messages SET content = ?, updated_at = ?
             WHERE chat_id = ? AND created_at = ? AND message_id = ?",
            )
            .await?;

        let delete_stmt = session
            .prepare(
                "UPDATE messages SET is_deleted = true, updated_at = ?
             WHERE chat_id = ? AND created_at = ? AND message_id = ?",
            )
            .await?;

        Ok(Self {
            session,
            insert_stmt,
            get_by_id_stmt,
            get_by_chat_stmt,
            update_content_stmt,
            delete_stmt,
        })
    }

    pub async fn create_message(&self, chat_id: Uuid, user_id: Uuid, content: String) -> ScyllaResult<ChatMessage> {
        let message_id = Uuid::new_v4();
        let created_at = Utc::now();

        let message = ChatMessage {
            message_id,
            chat_id,
            user_id,
            content: content.clone(),
            created_at,
            updated_at: None,
            is_deleted: false,
        };

        self.session
            .execute_unpaged(
                &self.insert_stmt,
                (
                    &message.message_id,
                    &message.chat_id,
                    &message.user_id,
                    &message.content,
                    created_at,
                    None::<DateTime<Utc>>,
                    false,
                ),
            )
            .await?;

        self.session
            .query_unpaged(
                "INSERT INTO user_messages (user_id, created_at, message_id, chat_id) VALUES (?, ?, ?, ?)",
                (&message.user_id, created_at, &message.message_id, &message.chat_id),
            )
            .await?;

        Ok(message)
    }

    pub async fn get_message(&self, message_id: Uuid) -> ScyllaResult<Option<ChatMessage>> {
        let query_result = self.session.execute_unpaged(&self.get_by_id_stmt, (message_id,)).await?;
        let rows_result = query_result.into_rows_result()?;

        if let Some((message_id, chat_id, user_id, content, created_at, updated_at, is_deleted)) =
            rows_result.maybe_first_row::<(Uuid, Uuid, Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>, bool)>()?
        {
            let message = ChatMessage {
                message_id,
                chat_id,
                user_id,
                content,
                created_at,
                updated_at,
                is_deleted,
            };
            return Ok(Some(message));
        }

        Ok(None)
    }

    pub async fn get_chat_messages(&self, chat_id: Uuid, limit: i32) -> ScyllaResult<Vec<ChatMessage>> {
        let query_result = self.session.execute_unpaged(&self.get_by_chat_stmt, (chat_id, limit)).await?;

        let rows_result = query_result.into_rows_result()?;
        let mut messages = Vec::new();

        for row in rows_result.rows::<(Uuid, Uuid, Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>, bool)>()? {
            let (message_id, chat_id, user_id, content, created_at, updated_at, is_deleted) = row?;

            messages.push(ChatMessage {
                message_id,
                chat_id,
                user_id,
                content,
                created_at,
                updated_at,
                is_deleted,
            });
        }

        Ok(messages)
    }

    pub async fn update_message(&self, message_id: Uuid, new_content: String) -> ScyllaResult<()> {
        if let Some(message) = self.get_message(message_id).await? {
            let updated_millis = Utc::now().timestamp_millis();
            let updated_timestamp = CqlTimestamp(updated_millis);
            let created_millis = message.created_at.timestamp_millis();
            let created_timestamp = CqlTimestamp(created_millis);

            self.session
                .execute_unpaged(
                    &self.update_content_stmt,
                    (
                        new_content.as_str(),
                        updated_timestamp,
                        message.chat_id,
                        created_timestamp,
                        message_id,
                    ),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn delete_message(&self, message_id: Uuid) -> ScyllaResult<()> {
        if let Some(message) = self.get_message(message_id).await? {
            let updated_millis = Utc::now().timestamp_millis();
            let updated_timestamp = CqlTimestamp(updated_millis);
            let created_millis = message.created_at.timestamp_millis();
            let created_timestamp = CqlTimestamp(created_millis);

            self.session
                .execute_unpaged(
                    &self.delete_stmt,
                    (updated_timestamp, message.chat_id, created_timestamp, message_id),
                )
                .await?;
        }

        Ok(())
    }

    pub fn get_metrics(&self) -> Arc<Metrics> {
        self.session.get_metrics()
    }
}

impl Drop for ChatMessageStore {
    fn drop(&mut self) {
        tracing::info!("Closing ScyllaDB connection");
    }
}
