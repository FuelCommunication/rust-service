pub mod config;
pub mod error;
mod lock;
mod pubsub;

pub use config::ValkeyConfig;
pub use error::{ValkeyError, ValkeyResult};
pub use lock::Lock;
pub use pubsub::{PubSubMessage, Subscription};
pub use redis;

use redis::{
    AsyncCommands, Client, FromRedisValue, ToRedisArgs,
    aio::{ConnectionManager, ConnectionManagerConfig},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{collections::HashMap, future::Future, sync::Arc, time::Duration};

const SCAN_BATCH: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ttl {
    Missing,
    Persistent,
    Expires(Duration),
}

#[derive(Clone)]
pub struct Valkey {
    client: Client,
    conn: ConnectionManager,
    prefix: Option<Arc<str>>,
}

impl std::fmt::Debug for Valkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Valkey").field("prefix", &self.prefix).finish_non_exhaustive()
    }
}

fn millis(ttl: Duration) -> u64 {
    u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX).max(1)
}

fn encode<T: Serialize + ?Sized>(key: &str, value: &T) -> ValkeyResult<String> {
    serde_json::to_string(value).map_err(|e| ValkeyError::encode(key, e))
}

fn decode<T: DeserializeOwned>(key: &str, raw: &str) -> ValkeyResult<T> {
    serde_json::from_str(raw).map_err(|e| ValkeyError::decode(key, e))
}

fn decode_opt<T: DeserializeOwned>(key: &str, raw: Option<String>) -> ValkeyResult<Option<T>> {
    raw.map(|raw| decode(key, &raw)).transpose()
}

fn glob_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

impl Valkey {
    pub async fn new(config: ValkeyConfig) -> ValkeyResult<Self> {
        let client = Client::open(config.url.as_str())?;
        let manager_config = ConnectionManagerConfig::new()
            .set_connection_timeout(Some(config.connection_timeout))
            .set_response_timeout(Some(config.response_timeout))
            .set_number_of_retries(config.reconnect_retries)
            .set_max_delay(config.reconnect_max_delay);

        let conn = if config.lazy {
            ConnectionManager::new_lazy_with_config(client.clone(), manager_config)?
        } else {
            ConnectionManager::new_with_config(client.clone(), manager_config).await?
        };

        tracing::info!(
            url = %config::redact_url(&config.url),
            prefix = config.key_prefix.as_deref(),
            lazy = config.lazy,
            "Valkey client ready"
        );
        Ok(Self {
            client,
            conn,
            prefix: config.key_prefix.map(Arc::from),
        })
    }

    pub async fn connect(url: &str) -> ValkeyResult<Self> {
        Self::new(ValkeyConfig::new(url)).await
    }

    pub fn prefixed_key(&self, key: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{prefix}:{key}"),
            None => key.to_owned(),
        }
    }

    fn prefixed_keys(&self, keys: &[&str]) -> Vec<String> {
        keys.iter().map(|k| self.prefixed_key(k)).collect()
    }

    fn strip_prefix<'a>(&self, key: &'a str) -> &'a str {
        match &self.prefix {
            Some(prefix) => key.strip_prefix(&**prefix).and_then(|k| k.strip_prefix(':')).unwrap_or(key),
            None => key,
        }
    }

    fn prefixed_pattern(&self, pattern: &str) -> String {
        match &self.prefix {
            Some(prefix) => format!("{}:{pattern}", glob_escape(prefix)),
            None => pattern.to_owned(),
        }
    }

    fn conn(&self) -> ConnectionManager {
        self.conn.clone()
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn().get(self.prefixed_key(key)).await?;
        decode_opt(key, raw)
    }

    pub async fn set<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> ValkeyResult<()> {
        let json = encode(key, value)?;
        let _: () = self.conn().set(self.prefixed_key(key), json).await?;
        Ok(())
    }

    pub async fn set_ex<T: Serialize + ?Sized>(&self, key: &str, value: &T, ttl: Duration) -> ValkeyResult<()> {
        let json = encode(key, value)?;
        let _: () = redis::cmd("SET")
            .arg(self.prefixed_key(key))
            .arg(json)
            .arg("PX")
            .arg(millis(ttl))
            .query_async(&mut self.conn())
            .await?;
        Ok(())
    }

    pub async fn set_nx<T: Serialize + ?Sized>(&self, key: &str, value: &T, ttl: Option<Duration>) -> ValkeyResult<bool> {
        let json = encode(key, value)?;
        let mut cmd = redis::cmd("SET");
        cmd.arg(self.prefixed_key(key)).arg(json).arg("NX");
        if let Some(ttl) = ttl {
            cmd.arg("PX").arg(millis(ttl));
        }
        let reply: Option<String> = cmd.query_async(&mut self.conn()).await?;
        Ok(reply.is_some())
    }

    pub async fn get_del<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn().get_del(self.prefixed_key(key)).await?;
        decode_opt(key, raw)
    }

    pub async fn mget<T: DeserializeOwned>(&self, keys: &[&str]) -> ValkeyResult<Vec<Option<T>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let raw: Vec<Option<String>> = redis::cmd("MGET")
            .arg(self.prefixed_keys(keys))
            .query_async(&mut self.conn())
            .await?;
        keys.iter().zip(raw).map(|(key, raw)| decode_opt(key, raw)).collect()
    }

    pub async fn mset<T: Serialize>(&self, pairs: &[(&str, &T)]) -> ValkeyResult<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        let items = pairs
            .iter()
            .map(|(key, value)| Ok((self.prefixed_key(key), encode(key, value)?)))
            .collect::<ValkeyResult<Vec<_>>>()?;
        let _: () = self.conn().mset(&items).await?;
        Ok(())
    }

    pub async fn mset_ex<T: Serialize>(&self, pairs: &[(&str, &T)], ttl: Duration) -> ValkeyResult<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        let mut pipe = redis::pipe();
        pipe.atomic();
        for (key, value) in pairs {
            pipe.cmd("SET")
                .arg(self.prefixed_key(key))
                .arg(encode(key, value)?)
                .arg("PX")
                .arg(millis(ttl))
                .ignore();
        }
        let _: () = pipe.query_async(&mut self.conn()).await?;
        Ok(())
    }

    pub async fn get_or_set<T, E, F, Fut>(&self, key: &str, ttl: Duration, load: F) -> Result<T, E>
    where
        T: Serialize + DeserializeOwned,
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        match self.get::<T>(key).await {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(error) => tracing::warn!(key, %error, "Cache read failed, loading from source"),
        }

        let value = load().await?;
        if let Err(error) = self.set_ex(key, &value, ttl).await {
            tracing::warn!(key, %error, "Cache write failed");
        }
        Ok(value)
    }

    pub async fn get_raw(&self, key: &str) -> ValkeyResult<Option<Vec<u8>>> {
        Ok(self.conn().get(self.prefixed_key(key)).await?)
    }

    pub async fn set_raw(&self, key: &str, value: &[u8]) -> ValkeyResult<()> {
        let _: () = self.conn().set(self.prefixed_key(key), value).await?;
        Ok(())
    }

    pub async fn set_raw_ex(&self, key: &str, value: &[u8], ttl: Duration) -> ValkeyResult<()> {
        let _: () = redis::cmd("SET")
            .arg(self.prefixed_key(key))
            .arg(value)
            .arg("PX")
            .arg(millis(ttl))
            .query_async(&mut self.conn())
            .await?;
        Ok(())
    }

    pub async fn incr(&self, key: &str, delta: i64) -> ValkeyResult<i64> {
        Ok(self.conn().incr(self.prefixed_key(key), delta).await?)
    }

    pub async fn incr_with_ttl(&self, key: &str, delta: i64, ttl: Duration) -> ValkeyResult<i64> {
        let key = self.prefixed_key(key);
        let (value, _): (i64, i64) = redis::pipe()
            .atomic()
            .cmd("INCRBY")
            .arg(&key)
            .arg(delta)
            .cmd("PEXPIRE")
            .arg(&key)
            .arg(millis(ttl))
            .arg("NX")
            .query_async(&mut self.conn())
            .await?;
        Ok(value)
    }

    pub async fn hset<T: Serialize + ?Sized>(&self, key: &str, field: &str, value: &T) -> ValkeyResult<()> {
        let json = encode(key, value)?;
        let _: () = self.conn().hset(self.prefixed_key(key), field, json).await?;
        Ok(())
    }

    pub async fn hget<T: DeserializeOwned>(&self, key: &str, field: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn().hget(self.prefixed_key(key), field).await?;
        decode_opt(key, raw)
    }

    pub async fn hgetall<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<HashMap<String, T>> {
        let raw: HashMap<String, String> = self.conn().hgetall(self.prefixed_key(key)).await?;
        raw.into_iter()
            .map(|(field, json)| Ok((field, decode(key, &json)?)))
            .collect()
    }

    pub async fn hmset<T: Serialize>(&self, key: &str, fields: &[(&str, &T)]) -> ValkeyResult<()> {
        if fields.is_empty() {
            return Ok(());
        }
        let items = fields
            .iter()
            .map(|(field, value)| Ok((*field, encode(key, value)?)))
            .collect::<ValkeyResult<Vec<_>>>()?;
        let _: () = self.conn().hset_multiple(self.prefixed_key(key), &items).await?;
        Ok(())
    }

    pub async fn hdel(&self, key: &str, fields: &[&str]) -> ValkeyResult<u64> {
        if fields.is_empty() {
            return Ok(0);
        }
        Ok(self.conn().hdel(self.prefixed_key(key), fields).await?)
    }

    pub async fn hexists(&self, key: &str, field: &str) -> ValkeyResult<bool> {
        Ok(self.conn().hexists(self.prefixed_key(key), field).await?)
    }

    pub async fn hincr(&self, key: &str, field: &str, delta: i64) -> ValkeyResult<i64> {
        Ok(self.conn().hincr(self.prefixed_key(key), field, delta).await?)
    }

    pub async fn lpush<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> ValkeyResult<u64> {
        let json = encode(key, value)?;
        Ok(self.conn().lpush(self.prefixed_key(key), json).await?)
    }

    pub async fn rpush<T: Serialize + ?Sized>(&self, key: &str, value: &T) -> ValkeyResult<u64> {
        let json = encode(key, value)?;
        Ok(self.conn().rpush(self.prefixed_key(key), json).await?)
    }

    pub async fn lpop<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn().lpop(self.prefixed_key(key), None).await?;
        decode_opt(key, raw)
    }

    pub async fn rpop<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn().rpop(self.prefixed_key(key), None).await?;
        decode_opt(key, raw)
    }

    pub async fn lrange<T: DeserializeOwned>(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<T>> {
        let raw: Vec<String> = self.conn().lrange(self.prefixed_key(key), start, stop).await?;
        raw.iter().map(|json| decode(key, json)).collect()
    }

    pub async fn ltrim(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<()> {
        let _: () = self.conn().ltrim(self.prefixed_key(key), start, stop).await?;
        Ok(())
    }

    pub async fn llen(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn().llen(self.prefixed_key(key)).await?)
    }

    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> ValkeyResult<bool> {
        let added: i64 = self.conn().zadd(self.prefixed_key(key), member, score).await?;
        Ok(added > 0)
    }

    pub async fn zrem(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn().zrem(self.prefixed_key(key), members).await?)
    }

    pub async fn zrange(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<String>> {
        Ok(self.conn().zrange(self.prefixed_key(key), start, stop).await?)
    }

    pub async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<(String, f64)>> {
        Ok(self.conn().zrange_withscores(self.prefixed_key(key), start, stop).await?)
    }

    pub async fn zrevrange(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<String>> {
        Ok(self.conn().zrevrange(self.prefixed_key(key), start, stop).await?)
    }

    pub async fn zrangebyscore(&self, key: &str, min: f64, max: f64) -> ValkeyResult<Vec<String>> {
        Ok(self.conn().zrangebyscore(self.prefixed_key(key), min, max).await?)
    }

    pub async fn zremrangebyscore(&self, key: &str, min: f64, max: f64) -> ValkeyResult<u64> {
        Ok(self.conn().zrembyscore(self.prefixed_key(key), min, max).await?)
    }

    pub async fn zscore(&self, key: &str, member: &str) -> ValkeyResult<Option<f64>> {
        Ok(self.conn().zscore(self.prefixed_key(key), member).await?)
    }

    pub async fn zcard(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn().zcard(self.prefixed_key(key)).await?)
    }

    pub async fn zincrby(&self, key: &str, member: &str, delta: f64) -> ValkeyResult<f64> {
        Ok(self.conn().zincr(self.prefixed_key(key), member, delta).await?)
    }

    pub async fn sadd(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn().sadd(self.prefixed_key(key), members).await?)
    }

    pub async fn srem(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn().srem(self.prefixed_key(key), members).await?)
    }

    pub async fn smembers(&self, key: &str) -> ValkeyResult<Vec<String>> {
        Ok(self.conn().smembers(self.prefixed_key(key)).await?)
    }

    pub async fn sismember(&self, key: &str, member: &str) -> ValkeyResult<bool> {
        Ok(self.conn().sismember(self.prefixed_key(key), member).await?)
    }

    pub async fn scard(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn().scard(self.prefixed_key(key)).await?)
    }

    pub async fn del(&self, key: &str) -> ValkeyResult<bool> {
        let removed: u64 = self.conn().del(self.prefixed_key(key)).await?;
        Ok(removed > 0)
    }

    pub async fn del_many(&self, keys: &[&str]) -> ValkeyResult<u64> {
        if keys.is_empty() {
            return Ok(0);
        }
        Ok(self.conn().del(self.prefixed_keys(keys)).await?)
    }

    pub async fn scan_keys(&self, pattern: &str) -> ValkeyResult<Vec<String>> {
        let pattern = self.prefixed_pattern(pattern);
        let mut conn = self.conn();
        let mut keys = Vec::new();
        let mut cursor = 0;
        loop {
            let (next, batch) = scan_page(&mut conn, cursor, &pattern).await?;
            keys.extend(batch.iter().map(|k| self.strip_prefix(k).to_owned()));
            if next == 0 {
                return Ok(keys);
            }
            cursor = next;
        }
    }

    pub async fn del_pattern(&self, pattern: &str) -> ValkeyResult<u64> {
        let pattern = self.prefixed_pattern(pattern);
        let mut conn = self.conn();
        let mut total = 0;
        let mut cursor = 0;
        loop {
            let (next, batch) = scan_page(&mut conn, cursor, &pattern).await?;
            if !batch.is_empty() {
                let removed: u64 = redis::cmd("UNLINK").arg(&batch).query_async(&mut conn).await?;
                total += removed;
            }
            if next == 0 {
                return Ok(total);
            }
            cursor = next;
        }
    }

    pub async fn exists(&self, key: &str) -> ValkeyResult<bool> {
        Ok(self.conn().exists(self.prefixed_key(key)).await?)
    }

    pub async fn expire(&self, key: &str, ttl: Duration) -> ValkeyResult<bool> {
        let set: i64 = redis::cmd("PEXPIRE")
            .arg(self.prefixed_key(key))
            .arg(millis(ttl))
            .query_async(&mut self.conn())
            .await?;
        Ok(set == 1)
    }

    pub async fn persist(&self, key: &str) -> ValkeyResult<bool> {
        Ok(self.conn().persist(self.prefixed_key(key)).await?)
    }

    pub async fn ttl(&self, key: &str) -> ValkeyResult<Ttl> {
        let ms: i64 = redis::cmd("PTTL")
            .arg(self.prefixed_key(key))
            .query_async(&mut self.conn())
            .await?;
        Ok(match ms {
            -2 => Ttl::Missing,
            -1 => Ttl::Persistent,
            ms => Ttl::Expires(Duration::from_millis(ms.max(0) as u64)),
        })
    }

    pub async fn publish<T: Serialize + ?Sized>(&self, channel: &str, message: &T) -> ValkeyResult<u64> {
        let json = encode(channel, message)?;
        self.publish_raw(channel, json.as_bytes()).await
    }

    pub async fn publish_raw(&self, channel: &str, message: &[u8]) -> ValkeyResult<u64> {
        Ok(self.conn().publish(self.prefixed_key(channel), message).await?)
    }

    pub async fn subscribe(&self, channels: &[&str]) -> ValkeyResult<Subscription> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(self.prefixed_keys(channels)).await?;
        Ok(Subscription::new(pubsub, self.prefix.clone()))
    }

    pub async fn psubscribe(&self, patterns: &[&str]) -> ValkeyResult<Subscription> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        let patterns: Vec<String> = patterns.iter().map(|p| self.prefixed_pattern(p)).collect();
        pubsub.psubscribe(patterns).await?;
        Ok(Subscription::new(pubsub, self.prefix.clone()))
    }

    pub async fn query<T: FromRedisValue>(&self, cmd: &redis::Cmd) -> ValkeyResult<T> {
        Ok(cmd.query_async(&mut self.conn()).await?)
    }

    pub async fn pipeline<T: FromRedisValue>(&self, pipe: &redis::Pipeline) -> ValkeyResult<T> {
        Ok(pipe.query_async(&mut self.conn()).await?)
    }

    pub async fn eval<T: FromRedisValue>(
        &self,
        script: &redis::Script,
        keys: &[&str],
        args: impl ToRedisArgs,
    ) -> ValkeyResult<T> {
        let mut invocation = script.prepare_invoke();
        for key in keys {
            invocation.key(self.prefixed_key(key));
        }
        invocation.arg(args);
        Ok(invocation.invoke_async(&mut self.conn()).await?)
    }

    pub fn connection(&self) -> ConnectionManager {
        self.conn()
    }

    pub async fn ping(&self) -> ValkeyResult<()> {
        let _: String = redis::cmd("PING").query_async(&mut self.conn()).await?;
        Ok(())
    }

    pub async fn dbsize(&self) -> ValkeyResult<u64> {
        Ok(redis::cmd("DBSIZE").query_async(&mut self.conn()).await?)
    }

    pub async fn flush_db(&self) -> ValkeyResult<()> {
        Ok(redis::cmd("FLUSHDB").query_async(&mut self.conn()).await?)
    }
}

async fn scan_page(conn: &mut ConnectionManager, cursor: u64, pattern: &str) -> ValkeyResult<(u64, Vec<String>)> {
    Ok(redis::cmd("SCAN")
        .arg(cursor)
        .arg("MATCH")
        .arg(pattern)
        .arg("COUNT")
        .arg(SCAN_BATCH)
        .query_async(conn)
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_glob_characters() {
        assert_eq!(glob_escape("app*[1]?\\"), "app\\*\\[1\\]\\?\\\\");
        assert_eq!(glob_escape("plain:prefix"), "plain:prefix");
    }

    #[test]
    fn ttl_is_never_zero() {
        assert_eq!(millis(Duration::ZERO), 1);
        assert_eq!(millis(Duration::from_secs(2)), 2000);
        assert_eq!(millis(Duration::MAX), u64::MAX);
    }
}
