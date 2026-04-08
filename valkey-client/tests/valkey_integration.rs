use serde::{Deserialize, Serialize};
use testcontainers_modules::{
    testcontainers::runners::AsyncRunner as _,
    valkey::{VALKEY_PORT, Valkey},
};
use valkey_client::Valkey as ValkeyClient;

async fn setup() -> anyhow::Result<(testcontainers_modules::testcontainers::ContainerAsync<Valkey>, ValkeyClient)> {
    let container = Valkey::default().start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(VALKEY_PORT).await?;
    let url = format!("redis://{}:{}", host, port);
    let client = ValkeyClient::new(&url).await?;
    Ok((container, client))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

#[tokio::test]
async fn test_ping() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;
    assert!(client.ping().await?);
    Ok(())
}

#[tokio::test]
async fn test_set_get_typed() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let user = User {
        id: 1,
        name: "Alice".into(),
        email: "alice@test.com".into(),
    };
    client.set("user:1", &user).await?;

    let result: Option<User> = client.get("user:1").await?;
    assert_eq!(result, Some(user));

    let missing: Option<User> = client.get("user:999").await?;
    assert_eq!(missing, None);

    Ok(())
}

#[tokio::test]
async fn test_set_ex() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_ex("temp", &"value", 60).await?;
    let ttl = client.ttl("temp").await?;
    assert!(ttl > 0 && ttl <= 60);

    let result: Option<String> = client.get("temp").await?;
    assert_eq!(result, Some("value".into()));

    Ok(())
}

#[tokio::test]
async fn test_set_nx() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let first = client.set_nx("lock", &"owner1", 60).await?;
    assert!(first);

    let second = client.set_nx("lock", &"owner2", 60).await?;
    assert!(!second);

    let val: Option<String> = client.get("lock").await?;
    assert_eq!(val, Some("owner1".into()));

    Ok(())
}

#[tokio::test]
async fn test_mget_mset() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let a = 10i64;
    let b = 20i64;
    let c = 30i64;
    client.mset(&[("k1", &a), ("k2", &b), ("k3", &c)]).await?;

    let results: Vec<Option<i64>> = client.mget(&["k1", "k2", "missing", "k3"]).await?;
    assert_eq!(results, vec![Some(10), Some(20), None, Some(30)]);

    let empty: Vec<Option<i64>> = client.mget(&[]).await?;
    assert!(empty.is_empty());

    Ok(())
}

#[tokio::test]
async fn test_raw_set_get() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw("raw_key", "raw_value").await?;
    let val = client.get_raw("raw_key").await?;
    assert_eq!(val, Some("raw_value".into()));

    let missing = client.get_raw("no_such_key").await?;
    assert_eq!(missing, None);

    Ok(())
}

#[tokio::test]
async fn test_raw_set_ex() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw_ex("raw_ttl", "data", 30).await?;
    let ttl = client.ttl("raw_ttl").await?;
    assert!(ttl > 0 && ttl <= 30);

    Ok(())
}

#[tokio::test]
async fn test_hash_operations() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.hset("h", "name", &"Alice").await?;
    client.hset("h", "age", &25i64).await?;

    let name: Option<String> = client.hget("h", "name").await?;
    assert_eq!(name, Some("Alice".into()));

    let age: Option<i64> = client.hget("h", "age").await?;
    assert_eq!(age, Some(25));

    let missing: Option<String> = client.hget("h", "nope").await?;
    assert_eq!(missing, None);

    assert!(client.hexists("h", "name").await?);
    assert!(!client.hexists("h", "nope").await?);

    Ok(())
}

#[tokio::test]
async fn test_hgetall() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.hset("map", "a", &1i64).await?;
    client.hset("map", "b", &2i64).await?;

    let all: std::collections::HashMap<String, i64> = client.hgetall("map").await?;
    assert_eq!(all.len(), 2);
    assert_eq!(all["a"], 1);
    assert_eq!(all["b"], 2);

    Ok(())
}

#[tokio::test]
async fn test_hmset() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let v1 = "x".to_string();
    let v2 = "y".to_string();
    client.hmset("hm", &[("f1", &v1), ("f2", &v2)]).await?;

    let f1: Option<String> = client.hget("hm", "f1").await?;
    let f2: Option<String> = client.hget("hm", "f2").await?;
    assert_eq!(f1, Some("x".into()));
    assert_eq!(f2, Some("y".into()));

    Ok(())
}

#[tokio::test]
async fn test_hdel() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.hset("hd", "a", &1i64).await?;
    client.hset("hd", "b", &2i64).await?;

    let removed = client.hdel("hd", &["a", "missing"]).await?;
    assert_eq!(removed, 1);

    assert!(!client.hexists("hd", "a").await?);
    assert!(client.hexists("hd", "b").await?);
    assert_eq!(client.hdel("hd", &[]).await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_hincr() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let val = client.hincr("counters", "views", 5).await?;
    assert_eq!(val, 5);

    let val = client.hincr("counters", "views", -2).await?;
    assert_eq!(val, 3);

    Ok(())
}

#[tokio::test]
async fn test_list_push_pop() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.rpush("list", &"a").await?;
    client.rpush("list", &"b").await?;
    client.lpush("list", &"z").await?;

    assert_eq!(client.llen("list").await?, 3);

    let left: Option<String> = client.lpop("list").await?;
    assert_eq!(left, Some("z".into()));

    let right: Option<String> = client.rpop("list").await?;
    assert_eq!(right, Some("b".into()));

    assert_eq!(client.llen("list").await?, 1);

    Ok(())
}

#[tokio::test]
async fn test_lrange() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.rpush("lr", &1i64).await?;
    client.rpush("lr", &2i64).await?;
    client.rpush("lr", &3i64).await?;

    let all: Vec<i64> = client.lrange("lr", 0, -1).await?;
    assert_eq!(all, vec![1, 2, 3]);

    let sub: Vec<i64> = client.lrange("lr", 0, 1).await?;
    assert_eq!(sub, vec![1, 2]);

    Ok(())
}

#[tokio::test]
async fn test_lpop_rpop_empty() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let left: Option<String> = client.lpop("empty_list").await?;
    assert_eq!(left, None);

    let right: Option<String> = client.rpop("empty_list").await?;
    assert_eq!(right, None);

    Ok(())
}

#[tokio::test]
async fn test_sorted_set() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    assert!(client.zadd("zs", "alice", 100.0).await?);
    assert!(client.zadd("zs", "bob", 200.0).await?);
    assert!(client.zadd("zs", "charlie", 150.0).await?);

    assert_eq!(client.zcard("zs").await?, 3);

    let score = client.zscore("zs", "bob").await?;
    assert_eq!(score, Some(200.0));

    let asc = client.zrange("zs", 0, -1).await?;
    assert_eq!(asc, vec!["alice", "charlie", "bob"]);

    let desc = client.zrevrange("zs", 0, -1).await?;
    assert_eq!(desc, vec!["bob", "charlie", "alice"]);

    let with_scores = client.zrange_withscores("zs", 0, 1).await?;
    assert_eq!(with_scores, vec![("alice".into(), 100.0), ("charlie".into(), 150.0)]);

    Ok(())
}

#[tokio::test]
async fn test_zrem() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.zadd("zr", "a", 1.0).await?;
    client.zadd("zr", "b", 2.0).await?;

    let removed = client.zrem("zr", &["a", "missing"]).await?;
    assert_eq!(removed, 1);
    assert_eq!(client.zcard("zr").await?, 1);
    assert_eq!(client.zrem("zr", &[]).await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_zincrby() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.zadd("zi", "player", 10.0).await?;
    let new_score = client.zincrby("zi", "player", 5.5).await?;
    assert!((new_score - 15.5).abs() < f64::EPSILON);

    Ok(())
}

#[tokio::test]
async fn test_set_operations() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let added = client.sadd("s", &["a", "b", "c"]).await?;
    assert_eq!(added, 3);

    let added_again = client.sadd("s", &["b", "c", "d"]).await?;
    assert_eq!(added_again, 1);

    assert_eq!(client.scard("s").await?, 4);
    assert!(client.sismember("s", "a").await?);
    assert!(!client.sismember("s", "z").await?);

    let mut members = client.smembers("s").await?;
    members.sort();
    assert_eq!(members, vec!["a", "b", "c", "d"]);

    let removed = client.srem("s", &["a", "missing"]).await?;
    assert_eq!(removed, 1);
    assert_eq!(client.scard("s").await?, 3);

    assert_eq!(client.sadd("s", &[]).await?, 0);
    assert_eq!(client.srem("s", &[]).await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_del() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw("to_del", "v").await?;
    assert!(client.del("to_del").await?);
    assert!(!client.del("to_del").await?);
    assert!(!client.exists("to_del").await?);

    Ok(())
}

#[tokio::test]
async fn test_del_many() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw("dm1", "a").await?;
    client.set_raw("dm2", "b").await?;

    let removed = client.del_many(&["dm1", "dm2", "dm3"]).await?;
    assert_eq!(removed, 2);

    assert_eq!(client.del_many(&[]).await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_del_pattern() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw("cache:user:1", "a").await?;
    client.set_raw("cache:user:2", "b").await?;
    client.set_raw("cache:channel:1", "c").await?;
    client.set_raw("other", "d").await?;

    let removed = client.del_pattern("cache:user:*").await?;
    assert_eq!(removed, 2);

    assert!(!client.exists("cache:user:1").await?);
    assert!(client.exists("cache:channel:1").await?);
    assert!(client.exists("other").await?);

    Ok(())
}

#[tokio::test]
async fn test_exists() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;
    assert!(!client.exists("nope").await?);
    client.set_raw("yes", "1").await?;
    assert!(client.exists("yes").await?);

    Ok(())
}

#[tokio::test]
async fn test_expire_and_ttl() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    client.set_raw("exp", "v").await?;
    let ttl_before = client.ttl("exp").await?;
    assert_eq!(ttl_before, -1); // no expiry

    client.expire("exp", 120).await?;
    let ttl_after = client.ttl("exp").await?;
    assert!(ttl_after > 0 && ttl_after <= 120);

    let ttl_missing = client.ttl("no_key").await?;
    assert_eq!(ttl_missing, -2);

    Ok(())
}

#[tokio::test]
async fn test_incr() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    let val = client.incr("counter", 1).await?;
    assert_eq!(val, 1);

    let val = client.incr("counter", 10).await?;
    assert_eq!(val, 11);

    let val = client.incr("counter", -3).await?;
    assert_eq!(val, 8);

    Ok(())
}

#[tokio::test]
async fn test_dbsize_and_flush() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;

    assert_eq!(client.dbsize().await?, 0);
    client.set_raw("a", "1").await?;
    client.set_raw("b", "2").await?;
    assert_eq!(client.dbsize().await?, 2);

    client.flush_db().await?;
    assert_eq!(client.dbsize().await?, 0);

    Ok(())
}

#[tokio::test]
async fn test_publish_no_subscribers() -> anyhow::Result<()> {
    let (_c, client) = setup().await?;
    let receivers = client.publish("ch", "hello").await?;
    assert_eq!(receivers, 0);

    Ok(())
}
