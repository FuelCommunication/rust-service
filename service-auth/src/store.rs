use chrono::{DateTime, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AuthError;

pub struct AuthStore {
    pool: PgPool,
}

impl AuthStore {
    const MAX_REFRESH_TOKENS_PER_USER: i64 = 10;

    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn create_user(&self, email: String, username: String, password_hash: String) -> Result<StoredUser, AuthError> {
        let id = Uuid::now_v7();

        let row = sqlx::query!(
            r#"INSERT INTO users (id, email, username, password_hash)
               VALUES ($1, $2, $3, $4)
               RETURNING id, email, username, password_hash"#,
            id,
            email,
            username,
            password_hash,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => AuthError::UserAlreadyExists,
            _ => AuthError::Internal(e.to_string()),
        })?;

        Ok(StoredUser {
            id: row.id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
        })
    }

    pub async fn create_oauth_user(&self, email: String, username: String) -> Result<StoredUser, AuthError> {
        let id = Uuid::now_v7();

        let row = sqlx::query!(
            r#"INSERT INTO users (id, email, username)
               VALUES ($1, $2, $3)
               RETURNING id, email, username, password_hash"#,
            id,
            email,
            username,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.is_unique_violation() => AuthError::UserAlreadyExists,
            _ => AuthError::Internal(e.to_string()),
        })?;

        Ok(StoredUser {
            id: row.id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
        })
    }

    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<StoredUser>, AuthError> {
        let row = sqlx::query!(
            r#"SELECT id, email, username, password_hash FROM users WHERE email = $1"#,
            email,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(row.map(|r| StoredUser {
            id: r.id,
            email: r.email,
            username: r.username,
            password_hash: r.password_hash,
        }))
    }

    pub async fn find_user_by_id(&self, id: Uuid) -> Result<StoredUser, AuthError> {
        let row = sqlx::query!(r#"SELECT id, email, username, password_hash FROM users WHERE id = $1"#, id,)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| AuthError::Internal(e.to_string()))?
            .ok_or(AuthError::UserNotFound)?;

        Ok(StoredUser {
            id: row.id,
            email: row.email,
            username: row.username,
            password_hash: row.password_hash,
        })
    }

    pub async fn find_oauth_account(
        &self,
        provider: &str,
        provider_user_id: &str,
    ) -> Result<Option<StoredOAuthAccount>, AuthError> {
        let row = sqlx::query!(
            r#"SELECT id, user_id, provider, provider_user_id
               FROM oauth_accounts
               WHERE provider = $1 AND provider_user_id = $2"#,
            provider,
            provider_user_id,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(row.map(|r| StoredOAuthAccount {
            id: r.id,
            user_id: r.user_id,
            provider: r.provider,
            provider_user_id: r.provider_user_id,
        }))
    }

    pub async fn create_oauth_account(
        &self,
        user_id: Uuid,
        provider: String,
        provider_user_id: String,
        provider_email: Option<String>,
    ) -> Result<(), AuthError> {
        let id = Uuid::now_v7();

        sqlx::query!(
            r#"INSERT INTO oauth_accounts (id, user_id, provider, provider_user_id, provider_email)
               VALUES ($1, $2, $3, $4, $5)"#,
            id,
            user_id,
            provider,
            provider_user_id,
            provider_email as Option<String>,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn store_refresh_token(&self, jti: Uuid, user_id: Uuid, expires_at: DateTime<Utc>) -> Result<(), AuthError> {
        let mut tx = self.pool.begin().await.map_err(|e| AuthError::Internal(e.to_string()))?;

        let lock_key = user_id.as_u128() as i64;
        sqlx::query!("SELECT pg_advisory_xact_lock($1)", lock_key)
            .execute(&mut *tx)
            .await
            .map_err(|e| AuthError::Internal(e.to_string()))?;

        sqlx::query!(
            r#"DELETE FROM refresh_tokens WHERE jti IN (
                SELECT jti FROM refresh_tokens WHERE user_id = $1
                ORDER BY created_at DESC
                OFFSET $2
            )"#,
            user_id,
            Self::MAX_REFRESH_TOKENS_PER_USER - 1,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        sqlx::query!(
            r#"INSERT INTO refresh_tokens (jti, user_id, expires_at)
               VALUES ($1, $2, $3)"#,
            jti,
            user_id,
            expires_at,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?;

        tx.commit().await.map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(())
    }

    pub async fn consume_refresh_token(&self, jti: Uuid) -> Result<StoredRefreshToken, AuthError> {
        let row = sqlx::query!(
            r#"DELETE FROM refresh_tokens
               WHERE jti = $1 AND expires_at > now()
               RETURNING user_id, expires_at"#,
            jti,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AuthError::Internal(e.to_string()))?
        .ok_or(AuthError::TokenNotFound)?;

        Ok(StoredRefreshToken {
            user_id: row.user_id,
            expires_at: row.expires_at,
        })
    }

    pub async fn revoke_refresh_token(&self, jti: Uuid) -> Result<(), AuthError> {
        let result = sqlx::query!(r#"DELETE FROM refresh_tokens WHERE jti = $1"#, jti,)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Internal(e.to_string()))?;

        if result.rows_affected() == 0 {
            return Err(AuthError::TokenNotFound);
        }

        Ok(())
    }

    pub async fn cleanup_expired_tokens(&self) -> Result<u64, AuthError> {
        let result = sqlx::query!(r#"DELETE FROM refresh_tokens WHERE expires_at <= now()"#)
            .execute(&self.pool)
            .await
            .map_err(|e| AuthError::Internal(e.to_string()))?;

        Ok(result.rows_affected())
    }
}

#[derive(Clone, Debug)]
pub struct StoredUser {
    pub id: Uuid,
    pub email: String,
    pub username: String,
    pub password_hash: Option<String>,
}

#[derive(Clone, Debug)]
pub struct StoredRefreshToken {
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct StoredOAuthAccount {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: String,
    pub provider_user_id: String,
}
