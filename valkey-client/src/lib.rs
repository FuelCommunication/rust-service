pub mod error;

use error::{ValkeyError, ValkeyResult};
use redis::{AsyncCommands, Client, Script, aio::ConnectionManager};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;

pub struct Valkey {
    conn: ConnectionManager,
}

impl Valkey {
    pub async fn new(url: &str) -> ValkeyResult<Self> {
        let client = Client::open(url)?;
        let conn = ConnectionManager::new(client).await?;

        tracing::info!("Valkey connection established");
        Ok(Self { conn })
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn.clone().get(key).await?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn set<T: Serialize>(&self, key: &str, value: &T) -> ValkeyResult<()> {
        let json = serde_json::to_string(value)?;
        let _: () = self.conn.clone().set(key, json).await?;
        Ok(())
    }

    pub async fn set_ex<T: Serialize>(&self, key: &str, value: &T, ttl_secs: u64) -> ValkeyResult<()> {
        let json = serde_json::to_string(value)?;
        let _: () = self.conn.clone().set_ex(key, json, ttl_secs).await?;
        Ok(())
    }

    pub async fn set_nx<T: Serialize>(&self, key: &str, value: &T, ttl_secs: u64) -> ValkeyResult<bool> {
        let json = serde_json::to_string(value)?;
        let set: bool = redis::cmd("SET")
            .arg(key)
            .arg(json)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut self.conn.clone())
            .await
            .unwrap_or(false);
        Ok(set)
    }

    pub async fn mget<T: DeserializeOwned>(&self, keys: &[&str]) -> ValkeyResult<Vec<Option<T>>> {
        if keys.is_empty() {
            return Ok(vec![]);
        }
        let raw: Vec<Option<String>> = self.conn.clone().mget(keys).await?;
        raw.into_iter()
            .map(|opt| match opt {
                Some(json) => Ok(Some(serde_json::from_str(&json)?)),
                None => Ok(None),
            })
            .collect()
    }

    pub async fn mset<T: Serialize>(&self, pairs: &[(&str, &T)]) -> ValkeyResult<()> {
        if pairs.is_empty() {
            return Ok(());
        }
        let serialized: Vec<(&str, String)> = pairs
            .iter()
            .map(|(k, v)| Ok((*k, serde_json::to_string(v)?)))
            .collect::<ValkeyResult<_>>()?;
        let refs: Vec<(&str, &str)> = serialized.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let _: () = self.conn.clone().mset(&refs).await?;
        Ok(())
    }

    pub async fn get_raw(&self, key: &str) -> ValkeyResult<Option<String>> {
        Ok(self.conn.clone().get(key).await?)
    }

    pub async fn set_raw(&self, key: &str, value: &str) -> ValkeyResult<()> {
        let _: () = self.conn.clone().set(key, value).await?;
        Ok(())
    }

    pub async fn set_raw_ex(&self, key: &str, value: &str, ttl_secs: u64) -> ValkeyResult<()> {
        let _: () = self.conn.clone().set_ex(key, value, ttl_secs).await?;
        Ok(())
    }

    pub async fn hset<T: Serialize>(&self, key: &str, field: &str, value: &T) -> ValkeyResult<()> {
        let json = serde_json::to_string(value)?;
        let _: () = self.conn.clone().hset(key, field, json).await?;
        Ok(())
    }

    pub async fn hget<T: DeserializeOwned>(&self, key: &str, field: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn.clone().hget(key, field).await?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn hgetall<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<HashMap<String, T>> {
        let raw: HashMap<String, String> = self.conn.clone().hgetall(key).await?;
        raw.into_iter().map(|(k, v)| Ok((k, serde_json::from_str(&v)?))).collect()
    }

    pub async fn hmset<T: Serialize>(&self, key: &str, fields: &[(&str, &T)]) -> ValkeyResult<()> {
        if fields.is_empty() {
            return Ok(());
        }
        let serialized: Vec<(&str, String)> = fields
            .iter()
            .map(|(f, v)| Ok((*f, serde_json::to_string(v)?)))
            .collect::<ValkeyResult<_>>()?;
        let refs: Vec<(&str, &str)> = serialized.iter().map(|(f, v)| (*f, v.as_str())).collect();
        let _: () = self.conn.clone().hset_multiple(key, &refs).await?;
        Ok(())
    }

    pub async fn hdel(&self, key: &str, fields: &[&str]) -> ValkeyResult<u64> {
        if fields.is_empty() {
            return Ok(0);
        }
        Ok(self.conn.clone().hdel(key, fields).await?)
    }

    pub async fn hexists(&self, key: &str, field: &str) -> ValkeyResult<bool> {
        Ok(self.conn.clone().hexists(key, field).await?)
    }

    pub async fn hincr(&self, key: &str, field: &str, delta: i64) -> ValkeyResult<i64> {
        Ok(self.conn.clone().hincr(key, field, delta).await?)
    }

    pub async fn lpush<T: Serialize>(&self, key: &str, value: &T) -> ValkeyResult<u64> {
        let json = serde_json::to_string(value)?;
        Ok(self.conn.clone().lpush(key, json).await?)
    }

    pub async fn rpush<T: Serialize>(&self, key: &str, value: &T) -> ValkeyResult<u64> {
        let json = serde_json::to_string(value)?;
        Ok(self.conn.clone().rpush(key, json).await?)
    }

    pub async fn lpop<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn.clone().lpop(key, None).await?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn rpop<T: DeserializeOwned>(&self, key: &str) -> ValkeyResult<Option<T>> {
        let raw: Option<String> = self.conn.clone().rpop(key, None).await?;
        match raw {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    pub async fn lrange<T: DeserializeOwned>(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<T>> {
        let raw: Vec<String> = self.conn.clone().lrange(key, start, stop).await?;
        raw.into_iter().map(|json| Ok(serde_json::from_str(&json)?)).collect()
    }

    pub async fn llen(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn.clone().llen(key).await?)
    }

    pub async fn zadd(&self, key: &str, member: &str, score: f64) -> ValkeyResult<bool> {
        let added: i64 = self.conn.clone().zadd(key, member, score).await?;
        Ok(added > 0)
    }

    pub async fn zrem(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn.clone().zrem(key, members).await?)
    }

    pub async fn zrange(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<String>> {
        Ok(self.conn.clone().zrange(key, start, stop).await?)
    }

    pub async fn zrange_withscores(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<(String, f64)>> {
        Ok(self.conn.clone().zrange_withscores(key, start, stop).await?)
    }

    pub async fn zrevrange(&self, key: &str, start: isize, stop: isize) -> ValkeyResult<Vec<String>> {
        Ok(self.conn.clone().zrevrange(key, start, stop).await?)
    }

    pub async fn zscore(&self, key: &str, member: &str) -> ValkeyResult<Option<f64>> {
        Ok(self.conn.clone().zscore(key, member).await?)
    }

    pub async fn zcard(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn.clone().zcard(key).await?)
    }

    pub async fn zincrby(&self, key: &str, member: &str, delta: f64) -> ValkeyResult<f64> {
        Ok(self.conn.clone().zincr(key, member, delta).await?)
    }

    pub async fn sadd(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn.clone().sadd(key, members).await?)
    }

    pub async fn srem(&self, key: &str, members: &[&str]) -> ValkeyResult<u64> {
        if members.is_empty() {
            return Ok(0);
        }
        Ok(self.conn.clone().srem(key, members).await?)
    }

    pub async fn smembers(&self, key: &str) -> ValkeyResult<Vec<String>> {
        Ok(self.conn.clone().smembers(key).await?)
    }

    pub async fn sismember(&self, key: &str, member: &str) -> ValkeyResult<bool> {
        Ok(self.conn.clone().sismember(key, member).await?)
    }

    pub async fn scard(&self, key: &str) -> ValkeyResult<u64> {
        Ok(self.conn.clone().scard(key).await?)
    }

    pub async fn del(&self, key: &str) -> ValkeyResult<bool> {
        let removed: i64 = self.conn.clone().del(key).await?;
        Ok(removed > 0)
    }

    pub async fn del_many(&self, keys: &[&str]) -> ValkeyResult<u64> {
        if keys.is_empty() {
            return Ok(0);
        }
        Ok(self.conn.clone().del(keys).await?)
    }

    pub async fn del_pattern(&self, pattern: &str) -> ValkeyResult<u64> {
        let script = Script::new(
            r#"
            local cursor = "0"
            local total = 0
            repeat
                local result = redis.call("SCAN", cursor, "MATCH", ARGV[1], "COUNT", 100)
                cursor = result[1]
                local keys = result[2]
                if #keys > 0 then
                    total = total + redis.call("DEL", unpack(keys))
                end
            until cursor == "0"
            return total
            "#,
        );
        let removed: u64 = script.arg(pattern).invoke_async(&mut self.conn.clone()).await?;
        Ok(removed)
    }

    pub async fn exists(&self, key: &str) -> ValkeyResult<bool> {
        Ok(self.conn.clone().exists(key).await?)
    }

    pub async fn expire(&self, key: &str, ttl_secs: u64) -> ValkeyResult<bool> {
        Ok(self.conn.clone().expire(key, ttl_secs as i64).await?)
    }

    pub async fn ttl(&self, key: &str) -> ValkeyResult<i64> {
        Ok(self.conn.clone().ttl(key).await?)
    }

    pub async fn incr(&self, key: &str, delta: i64) -> ValkeyResult<i64> {
        Ok(self.conn.clone().incr(key, delta).await?)
    }

    pub async fn publish(&self, channel: &str, message: &str) -> ValkeyResult<u64> {
        Ok(self.conn.clone().publish(channel, message).await?)
    }

    pub async fn flush_db(&self) -> ValkeyResult<()> {
        redis::cmd("FLUSHDB")
            .query_async(&mut self.conn.clone())
            .await
            .map_err(ValkeyError::Redis)
    }

    pub async fn ping(&self) -> ValkeyResult<bool> {
        let pong: String = redis::cmd("PING").query_async(&mut self.conn.clone()).await?;
        Ok(pong == "PONG")
    }

    pub async fn dbsize(&self) -> ValkeyResult<u64> {
        let size: u64 = redis::cmd("DBSIZE").query_async(&mut self.conn.clone()).await?;
        Ok(size)
    }
}
