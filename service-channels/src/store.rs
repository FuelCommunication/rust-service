use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    error::ApiError,
    models::{Channel, ChannelSubscriber, ChannelWithTotal, Subscription},
    schemas::{CreateChannel, UpdateChannel},
};

pub struct ChannelStore {
    pool: PgPool,
}

impl ChannelStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn list_channels(&self, limit: i64, offset: i64) -> Result<(Vec<ChannelWithTotal>, i64), ApiError> {
        let rows = sqlx::query_as!(
            ChannelWithTotal,
            r#"SELECT id, title, description, avatar_url,
                    COUNT(*) OVER() as "total!"
             FROM channels
             ORDER BY title
             LIMIT $1 OFFSET $2"#,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;

        let total = rows.first().map(|r| r.total).unwrap_or(0);
        Ok((rows, total))
    }

    pub async fn get_channel(&self, channel_id: Uuid) -> Result<Channel, ApiError> {
        sqlx::query_as!(
            Channel,
            r#"SELECT id, title, description, avatar_url, created_at, updated_at
             FROM channels
             WHERE id = $1"#,
            channel_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Channel with ID {channel_id} not found")))
    }

    pub async fn create_channel(&self, user_id: Uuid, data: CreateChannel) -> Result<Channel, ApiError> {
        let mut tx = self.pool.begin().await?;
        let channel_id = Uuid::now_v7();

        let channel = sqlx::query_as!(
            Channel,
            r#"INSERT INTO channels (id, title, description, avatar_url)
             VALUES ($1, $2, $3, $4)
             RETURNING id, title, description, avatar_url, created_at, updated_at"#,
            channel_id,
            data.title,
            data.description,
            data.avatar_url,
        )
        .fetch_one(&mut *tx)
        .await?;

        let sub_id = Uuid::now_v7();
        sqlx::query!(
            r#"INSERT INTO channel_subscribers (id, user_id, channel_id, is_owner)
             VALUES ($1, $2, $3, true)"#,
            sub_id,
            user_id,
            channel_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(channel)
    }

    pub async fn update_channel(&self, channel_id: Uuid, data: UpdateChannel) -> Result<Channel, ApiError> {
        let channel = sqlx::query_as!(
            Channel,
            r#"UPDATE channels
             SET title = COALESCE($1, title),
                 description = COALESCE($2, description),
                 avatar_url = COALESCE($3, avatar_url),
                 updated_at = NOW()
             WHERE id = $4
             RETURNING id, title, description, avatar_url, created_at, updated_at"#,
            data.title,
            data.description,
            data.avatar_url,
            channel_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("Channel with ID {channel_id} not found")))?;

        Ok(channel)
    }

    pub async fn delete_channel(&self, channel_id: Uuid) -> Result<(), ApiError> {
        let result = sqlx::query!(r#"DELETE FROM channels WHERE id = $1"#, channel_id)
            .execute(&self.pool)
            .await?;

        if result.rows_affected() == 0 {
            return Err(ApiError::NotFound(format!("Channel with ID {channel_id} not found")));
        }
        Ok(())
    }

    pub async fn get_user_subscriptions(
        &self,
        user_id: Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Subscription>, i64), ApiError> {
        let total = sqlx::query!(
            r#"SELECT COUNT(*) as "count!"
             FROM channels c
             INNER JOIN channel_subscribers cs ON cs.channel_id = c.id
             WHERE cs.user_id = $1"#,
            user_id,
        )
        .fetch_one(&self.pool)
        .await?
        .count;

        let items = sqlx::query_as!(
            Subscription,
            r#"SELECT c.id, c.title, c.description, c.avatar_url,
                    COUNT(cs_all.id) as "subscribers_count!"
             FROM channels c
             INNER JOIN channel_subscribers cs ON cs.channel_id = c.id AND cs.user_id = $1
             INNER JOIN channel_subscribers cs_all ON cs_all.channel_id = c.id
             GROUP BY c.id, c.title, c.description, c.avatar_url
             ORDER BY c.title
             LIMIT $2 OFFSET $3"#,
            user_id,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok((items, total))
    }

    pub async fn get_channel_subscribers(&self, channel_id: Uuid, limit: i64, offset: i64) -> Result<(Vec<Uuid>, i64), ApiError> {
        let total = sqlx::query!(
            r#"SELECT COUNT(*) as "count!"
             FROM channel_subscribers
             WHERE channel_id = $1"#,
            channel_id,
        )
        .fetch_one(&self.pool)
        .await?
        .count;

        let rows = sqlx::query!(
            r#"SELECT user_id
             FROM channel_subscribers
             WHERE channel_id = $1
             ORDER BY created_at
             LIMIT $2 OFFSET $3"#,
            channel_id,
            limit,
            offset,
        )
        .fetch_all(&self.pool)
        .await?;

        Ok((rows.into_iter().map(|r| r.user_id).collect(), total))
    }

    pub async fn get_all_subscriber_ids(&self, channel_id: Uuid) -> Result<Vec<Uuid>, ApiError> {
        let rows = sqlx::query!(r#"SELECT user_id FROM channel_subscribers WHERE channel_id = $1"#, channel_id,)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows.into_iter().map(|r| r.user_id).collect())
    }

    pub async fn is_owner(&self, user_id: Uuid, channel_id: Uuid) -> Result<bool, ApiError> {
        let row = sqlx::query!(
            r#"SELECT EXISTS(
                SELECT 1 FROM channel_subscribers
                WHERE user_id = $1 AND channel_id = $2 AND is_owner = true
             ) as "exists!""#,
            user_id,
            channel_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.exists)
    }

    pub async fn is_subscriber(&self, user_id: Uuid, channel_id: Uuid) -> Result<bool, ApiError> {
        let row = sqlx::query!(
            r#"SELECT EXISTS(
                SELECT 1 FROM channel_subscribers
                WHERE user_id = $1 AND channel_id = $2
             ) as "exists!""#,
            user_id,
            channel_id,
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(row.exists)
    }

    pub async fn subscribe(&self, user_id: Uuid, channel_id: Uuid) -> Result<(), ApiError> {
        let channel_exists = sqlx::query!(
            r#"SELECT EXISTS(SELECT 1 FROM channels WHERE id = $1) as "exists!""#,
            channel_id,
        )
        .fetch_one(&self.pool)
        .await?
        .exists;

        if !channel_exists {
            return Err(ApiError::NotFound("Channel not found".into()));
        }

        let sub_id = Uuid::now_v7();
        sqlx::query!(
            r#"INSERT INTO channel_subscribers (id, user_id, channel_id, is_owner)
             VALUES ($1, $2, $3, false)
             ON CONFLICT (user_id, channel_id) DO NOTHING"#,
            sub_id,
            user_id,
            channel_id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn unsubscribe(&self, user_id: Uuid, channel_id: Uuid) -> Result<(), ApiError> {
        let sub = sqlx::query_as!(
            ChannelSubscriber,
            r#"SELECT id, user_id, channel_id, is_owner
             FROM channel_subscribers
             WHERE user_id = $1 AND channel_id = $2"#,
            user_id,
            channel_id,
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| ApiError::NotFound("Subscription not found".into()))?;

        if sub.is_owner {
            return Err(ApiError::Conflict("Owner cannot unsubscribe from their own channel".into()));
        }

        sqlx::query!(r#"DELETE FROM channel_subscribers WHERE id = $1"#, sub.id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn transfer_ownership(&self, channel_id: Uuid, from_user_id: Uuid, to_user_id: Uuid) -> Result<(), ApiError> {
        if from_user_id == to_user_id {
            return Err(ApiError::BadRequest("Cannot transfer ownership to yourself".into()));
        }

        let mut tx = self.pool.begin().await?;

        let owner = sqlx::query_as!(
            ChannelSubscriber,
            r#"SELECT id, user_id, channel_id, is_owner
             FROM channel_subscribers
             WHERE user_id = $1 AND channel_id = $2 AND is_owner = true"#,
            from_user_id,
            channel_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::NotFound("You are not the owner of this channel".into()))?;

        let target = sqlx::query_as!(
            ChannelSubscriber,
            r#"SELECT id, user_id, channel_id, is_owner
             FROM channel_subscribers
             WHERE user_id = $1 AND channel_id = $2"#,
            to_user_id,
            channel_id,
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::NotFound("Target user is not a subscriber of this channel".into()))?;

        sqlx::query!(r#"UPDATE channel_subscribers SET is_owner = false WHERE id = $1"#, owner.id,)
            .execute(&mut *tx)
            .await?;

        sqlx::query!(r#"UPDATE channel_subscribers SET is_owner = true WHERE id = $1"#, target.id,)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(())
    }
}
