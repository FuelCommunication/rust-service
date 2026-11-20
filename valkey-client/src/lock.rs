use crate::{Valkey, ValkeyResult, millis};
use redis::Script;
use std::{
    hash::{BuildHasher, RandomState},
    sync::{
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static RELEASE: LazyLock<Script> = LazyLock::new(|| {
    Script::new(r#"if redis.call("GET", KEYS[1]) == ARGV[1] then return redis.call("DEL", KEYS[1]) else return 0 end"#)
});

static EXTEND: LazyLock<Script> = LazyLock::new(|| {
    Script::new(
        r#"if redis.call("GET", KEYS[1]) == ARGV[1] then return redis.call("PEXPIRE", KEYS[1], ARGV[2]) else return 0 end"#,
    )
});

fn token() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let random = RandomState::new().hash_one(nanos);
    format!(
        "{:x}-{:x}-{:x}-{random:x}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Debug)]
pub struct Lock {
    valkey: Valkey,
    key: String,
    token: String,
}

impl Lock {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub async fn release(self) -> ValkeyResult<bool> {
        let removed: i64 = self.valkey.eval(&RELEASE, &[&self.key], &self.token).await?;
        Ok(removed == 1)
    }

    pub async fn extend(&self, ttl: Duration) -> ValkeyResult<bool> {
        let extended: i64 = self.valkey.eval(&EXTEND, &[&self.key], (&self.token, millis(ttl))).await?;
        Ok(extended == 1)
    }
}

impl Valkey {
    pub async fn try_lock(&self, key: &str, ttl: Duration) -> ValkeyResult<Option<Lock>> {
        let token = token();
        let acquired = self.set_nx_raw(key, &token, ttl).await?;
        Ok(acquired.then(|| Lock {
            valkey: self.clone(),
            key: key.to_owned(),
            token,
        }))
    }

    pub async fn lock(&self, key: &str, ttl: Duration, wait: Duration, retry_interval: Duration) -> ValkeyResult<Option<Lock>> {
        let deadline = tokio::time::Instant::now() + wait;
        loop {
            if let Some(lock) = self.try_lock(key, ttl).await? {
                return Ok(Some(lock));
            }
            if tokio::time::Instant::now() + retry_interval > deadline {
                return Ok(None);
            }
            tokio::time::sleep(retry_interval).await;
        }
    }

    async fn set_nx_raw(&self, key: &str, value: &str, ttl: Duration) -> ValkeyResult<bool> {
        let reply: Option<String> = redis::cmd("SET")
            .arg(self.prefixed_key(key))
            .arg(value)
            .arg("NX")
            .arg("PX")
            .arg(millis(ttl))
            .query_async(&mut self.connection())
            .await?;
        Ok(reply.is_some())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn tokens_are_unique() {
        let a = super::token();
        let b = super::token();
        assert_ne!(a, b);
    }
}
