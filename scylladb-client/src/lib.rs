pub mod error;

use chrono::{DateTime, Utc};
use error::ScyllaResult;
pub use scylla::response::{PagingState, PagingStateResponse};
use scylla::{
    client::{execution_profile::ExecutionProfileBuilder, session::Session, session_builder::SessionBuilder},
    observability::metrics::Metrics,
    policies::retry::DefaultRetryPolicy,
    statement::{batch::Batch, prepared::PreparedStatement},
    value::CqlTimestamp,
};
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScyllaConfig {
    pub uri: String,
    pub additional_nodes: Vec<String>,
    pub connection_timeout: Duration,
    pub metadata_refresh_interval: Duration,
    pub keyspace: String,
    pub replication_factor: u8,
}

impl Default for ScyllaConfig {
    fn default() -> Self {
        Self {
            uri: "127.0.0.1:9042".into(),
            additional_nodes: Vec::new(),
            connection_timeout: Duration::from_secs(3),
            metadata_refresh_interval: Duration::from_secs(10),
            keyspace: "chat".into(),
            replication_factor: 3,
        }
    }
}

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
    insert_msg_stmt: PreparedStatement,
    insert_user_msg_stmt: PreparedStatement,
    insert_lookup_stmt: PreparedStatement,
    get_by_id_stmt: PreparedStatement,
    get_by_chat_stmt: PreparedStatement,
    update_content_stmt: PreparedStatement,
    delete_stmt: PreparedStatement,
}

impl ChatMessageStore {
    pub async fn new(config: &ScyllaConfig, run_migrations: bool) -> ScyllaResult<Self> {
        let profile = ExecutionProfileBuilder::default()
            .retry_policy(Arc::new(DefaultRetryPolicy::new()))
            .build();

        let mut builder = SessionBuilder::new()
            .known_node(&config.uri)
            .connection_timeout(config.connection_timeout)
            .cluster_metadata_refresh_interval(config.metadata_refresh_interval)
            .default_execution_profile_handle(profile.into_handle());

        for node in &config.additional_nodes {
            tracing::info!("Adding node: {}", node);
            builder = builder.known_node(node);
        }

        let session = Arc::new(builder.build().await?);

        if run_migrations {
            Self::migrate(&session, &config.keyspace, config.replication_factor).await?;
        }

        let store = Self::prepare(&session, &config.keyspace).await?;

        Ok(store)
    }

    pub async fn migrate(session: &Session, keyspace: &str, replication_factor: u8) -> ScyllaResult<()> {
        session
            .query_unpaged(
                format!(
                    "CREATE KEYSPACE IF NOT EXISTS {keyspace} \
                     WITH REPLICATION = {{'class': 'SimpleStrategy', 'replication_factor': {replication_factor}}}"
                ),
                &[],
            )
            .await?;

        session.query_unpaged(format!("USE {keyspace}"), &[]).await?;

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
            .query_unpaged(
                "CREATE TABLE IF NOT EXISTS message_by_id (
                    message_id UUID,
                    chat_id UUID,
                    created_at TIMESTAMP,
                    PRIMARY KEY (message_id)
                )",
                &[],
            )
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

        Ok(())
    }

    async fn prepare(session: &Arc<Session>, keyspace: &str) -> ScyllaResult<Self> {
        session.query_unpaged(format!("USE {keyspace}"), &[]).await?;

        let insert_msg_stmt = session
            .prepare(
                "INSERT INTO messages (message_id, chat_id, user_id, content, created_at, updated_at, is_deleted)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .await?;

        let insert_user_msg_stmt = session
            .prepare(
                "INSERT INTO user_messages (user_id, created_at, message_id, chat_id)
                 VALUES (?, ?, ?, ?)",
            )
            .await?;

        let insert_lookup_stmt = session
            .prepare(
                "INSERT INTO message_by_id (message_id, chat_id, created_at)
                 VALUES (?, ?, ?)",
            )
            .await?;

        let get_by_id_stmt = session
            .prepare("SELECT chat_id, created_at FROM message_by_id WHERE message_id = ?")
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
            session: Arc::clone(session),
            insert_msg_stmt,
            insert_user_msg_stmt,
            insert_lookup_stmt,
            get_by_id_stmt,
            get_by_chat_stmt,
            update_content_stmt,
            delete_stmt,
        })
    }

    pub async fn create_message(&self, chat_id: Uuid, user_id: Uuid, content: String) -> ScyllaResult<ChatMessage> {
        let message_id = Uuid::new_v4();
        let created_at = Utc::now();
        let created_ts = CqlTimestamp(created_at.timestamp_millis());

        let message = ChatMessage {
            message_id,
            chat_id,
            user_id,
            content: content.clone(),
            created_at,
            updated_at: None,
            is_deleted: false,
        };

        let mut batch = Batch::default();
        batch.append_statement(self.insert_msg_stmt.clone());
        batch.append_statement(self.insert_user_msg_stmt.clone());
        batch.append_statement(self.insert_lookup_stmt.clone());

        let batch_values = (
            (
                message_id,
                chat_id,
                user_id,
                content.as_str(),
                created_ts,
                None::<CqlTimestamp>,
                false,
            ),
            (user_id, created_ts, message_id, chat_id),
            (message_id, chat_id, created_ts),
        );

        self.session.batch(&batch, &batch_values).await?;

        Ok(message)
    }

    pub async fn get_message(&self, message_id: Uuid) -> ScyllaResult<Option<ChatMessage>> {
        let lookup_result = self.session.execute_unpaged(&self.get_by_id_stmt, (message_id,)).await?;
        let lookup_rows = lookup_result.into_rows_result()?;

        let Some((chat_id, created_ts)) = lookup_rows.maybe_first_row::<(Uuid, DateTime<Utc>)>()? else {
            return Ok(None);
        };

        let created_cql = CqlTimestamp(created_ts.timestamp_millis());

        let msg_result = self
            .session
            .query_unpaged(
                "SELECT message_id, chat_id, user_id, content, created_at, updated_at, is_deleted
                 FROM messages WHERE chat_id = ? AND created_at = ? AND message_id = ?",
                (chat_id, created_cql, message_id),
            )
            .await?;

        let msg_rows = msg_result.into_rows_result()?;

        if let Some((message_id, chat_id, user_id, content, created_at, updated_at, is_deleted)) =
            msg_rows.maybe_first_row::<(Uuid, Uuid, Uuid, String, DateTime<Utc>, Option<DateTime<Utc>>, bool)>()?
        {
            return Ok(Some(ChatMessage {
                message_id,
                chat_id,
                user_id,
                content,
                created_at,
                updated_at,
                is_deleted,
            }));
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

    pub async fn get_chat_messages_paged(
        &self,
        chat_id: Uuid,
        page_size: i32,
        paging_state: PagingState,
    ) -> ScyllaResult<(Vec<ChatMessage>, PagingStateResponse)> {
        let mut stmt = self.get_by_chat_stmt.clone();
        stmt.set_page_size(page_size);

        let (query_result, paging_response) = self
            .session
            .execute_single_page(&stmt, (chat_id, page_size), paging_state)
            .await?;

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

        Ok((messages, paging_response))
    }

    pub async fn update_message(
        &self,
        chat_id: Uuid,
        created_at: DateTime<Utc>,
        message_id: Uuid,
        new_content: String,
    ) -> ScyllaResult<()> {
        let updated_ts = CqlTimestamp(Utc::now().timestamp_millis());
        let created_ts = CqlTimestamp(created_at.timestamp_millis());

        self.session
            .execute_unpaged(
                &self.update_content_stmt,
                (new_content.as_str(), updated_ts, chat_id, created_ts, message_id),
            )
            .await?;

        Ok(())
    }

    pub async fn delete_message(&self, chat_id: Uuid, created_at: DateTime<Utc>, message_id: Uuid) -> ScyllaResult<()> {
        let updated_ts = CqlTimestamp(Utc::now().timestamp_millis());
        let created_ts = CqlTimestamp(created_at.timestamp_millis());

        self.session
            .execute_unpaged(&self.delete_stmt, (updated_ts, chat_id, created_ts, message_id))
            .await?;

        Ok(())
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    pub async fn health_check(&self) -> bool {
        self.session.query_unpaged("SELECT key FROM system.local", &[]).await.is_ok()
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
