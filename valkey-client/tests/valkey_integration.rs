use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};
use testcontainers_modules::{
    testcontainers::{ContainerAsync, runners::AsyncRunner as _},
    valkey::{VALKEY_PORT, Valkey as ValkeyImage},
};
use valkey_client::{Ttl, Valkey, ValkeyConfig, redis};

struct Ctx {
    container: ContainerAsync<ValkeyImage>,
    url: String,
    client: Valkey,
}

async fn setup() -> anyhow::Result<Ctx> {
    let container = ValkeyImage::default().start().await?;
    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(VALKEY_PORT).await?;
    let url = format!("redis://{host}:{port}");
    let client = Valkey::new(ValkeyConfig::new(&url).response_timeout(Duration::from_secs(10))).await?;
    Ok(Ctx { container, url, client })
}

impl Ctx {
    async fn with_prefix(&self, prefix: &str) -> anyhow::Result<Valkey> {
        let config = ValkeyConfig::new(&self.url)
            .key_prefix(prefix)
            .response_timeout(Duration::from_secs(10));
        Ok(Valkey::new(config).await?)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
}

fn alice() -> User {
    User {
        id: 1,
        name: "Alice".into(),
    }
}

#[tokio::test]
async fn ping_and_typed_values() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    client.ping().await?;

    client.set("user:1", &alice()).await?;
    assert_eq!(client.get::<User>("user:1").await?, Some(alice()));
    assert_eq!(client.get::<User>("user:404").await?, None);

    client.set("str", "plain").await?;
    assert_eq!(client.get::<String>("str").await?.as_deref(), Some("plain"));
    Ok(())
}

#[tokio::test]
async fn decode_error_carries_key() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    client.set("n", &42).await?;
    let err = client.get::<User>("n").await.unwrap_err();
    assert!(err.is_decode());
    assert!(err.to_string().contains("`n`"));
    Ok(())
}

#[tokio::test]
async fn set_ex_and_ttl() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;

    client.set_ex("temp", "value", Duration::from_secs(60)).await?;
    let Ttl::Expires(ttl) = client.ttl("temp").await? else {
        panic!("expected a TTL");
    };
    assert!(ttl > Duration::from_secs(55) && ttl <= Duration::from_secs(60));

    client.set("forever", &1).await?;
    assert_eq!(client.ttl("forever").await?, Ttl::Persistent);
    assert_eq!(client.ttl("missing").await?, Ttl::Missing);

    assert!(client.expire("forever", Duration::from_millis(1500)).await?);
    assert!(matches!(client.ttl("forever").await?, Ttl::Expires(_)));
    assert!(client.persist("forever").await?);
    assert_eq!(client.ttl("forever").await?, Ttl::Persistent);
    assert!(!client.expire("missing", Duration::from_secs(1)).await?);

    client.set_ex("short", &1, Duration::from_millis(100)).await?;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!client.exists("short").await?);
    Ok(())
}

#[tokio::test]
async fn set_nx() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;

    assert!(client.set_nx("once", "owner1", Some(Duration::from_secs(60))).await?);
    assert!(!client.set_nx("once", "owner2", Some(Duration::from_secs(60))).await?);
    assert_eq!(client.get::<String>("once").await?.as_deref(), Some("owner1"));

    assert!(client.set_nx("no-ttl", &1, None).await?);
    assert_eq!(client.ttl("no-ttl").await?, Ttl::Persistent);
    Ok(())
}

#[tokio::test]
async fn set_nx_reports_connection_errors() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let client = Valkey::new(
        ValkeyConfig::new(&ctx.url)
            .reconnect(0, Duration::from_millis(10))
            .connection_timeout(Duration::from_millis(300)),
    )
    .await?;
    ctx.container.stop().await?;

    let err = client.set_nx("k", &1, None).await.unwrap_err();
    assert!(err.is_connection_error() || err.is_timeout(), "{err}");
    Ok(())
}

#[tokio::test]
async fn get_del_mget_mset() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;

    client.mset(&[("a", &1), ("b", &2)]).await?;
    assert_eq!(
        client.mget::<i32>(&["a", "missing", "b"]).await?,
        vec![Some(1), None, Some(2)]
    );
    assert!(client.mget::<i32>(&[]).await?.is_empty());

    client.mset_ex(&[("x", &"1"), ("y", &"2")], Duration::from_secs(30)).await?;
    assert!(matches!(client.ttl("x").await?, Ttl::Expires(_)));
    assert!(matches!(client.ttl("y").await?, Ttl::Expires(_)));

    assert_eq!(client.get_del::<i32>("a").await?, Some(1));
    assert!(!client.exists("a").await?);
    Ok(())
}

#[tokio::test]
async fn raw_bytes() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    let bytes = [0u8, 159, 146, 150, 255];
    client.set_raw("bin", &bytes).await?;
    assert_eq!(client.get_raw("bin").await?.as_deref(), Some(&bytes[..]));

    client.set_raw_ex("bin2", b"x", Duration::from_secs(10)).await?;
    assert!(matches!(client.ttl("bin2").await?, Ttl::Expires(_)));
    assert_eq!(client.get_raw("missing").await?, None);
    Ok(())
}

#[tokio::test]
async fn counters() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    assert_eq!(client.incr("c", 1).await?, 1);
    assert_eq!(client.incr("c", 5).await?, 6);
    assert_eq!(client.incr("c", -2).await?, 4);

    let window = Duration::from_secs(60);
    assert_eq!(client.incr_with_ttl("rl", 1, window).await?, 1);
    let Ttl::Expires(first) = client.ttl("rl").await? else {
        panic!()
    };
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(client.incr_with_ttl("rl", 1, window).await?, 2);
    let Ttl::Expires(second) = client.ttl("rl").await? else {
        panic!()
    };
    assert!(second < first);
    Ok(())
}

#[tokio::test]
async fn get_or_set_loads_once() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    let calls = AtomicUsize::new(0);
    let load = || async {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::io::Error>(alice())
    };

    assert_eq!(client.get_or_set("cached", Duration::from_secs(60), load).await?, alice());
    assert_eq!(client.get_or_set("cached", Duration::from_secs(60), load).await?, alice());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(client.ttl("cached").await?, Ttl::Expires(_)));
    Ok(())
}

#[tokio::test]
async fn get_or_set_recovers_from_stale_schema_and_propagates_load_errors() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;

    client.set("stale", "not a user").await?;
    let user = client
        .get_or_set("stale", Duration::from_secs(60), || async {
            Ok::<_, std::io::Error>(alice())
        })
        .await?;
    assert_eq!(user, alice());
    assert_eq!(client.get::<User>("stale").await?, Some(alice()));

    let err = client
        .get_or_set::<User, _, _, _>("fails", Duration::from_secs(60), || async {
            Err(std::io::Error::other("db down"))
        })
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "db down");
    assert!(!client.exists("fails").await?);
    Ok(())
}

#[tokio::test]
async fn get_or_set_survives_dead_cache() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let client = Valkey::new(
        ValkeyConfig::new(&ctx.url)
            .reconnect(0, Duration::from_millis(10))
            .connection_timeout(Duration::from_millis(300))
            .response_timeout(Duration::from_millis(300)),
    )
    .await?;
    ctx.container.stop().await?;

    let user = client
        .get_or_set("k", Duration::from_secs(60), || async { Ok::<_, std::io::Error>(alice()) })
        .await?;
    assert_eq!(user, alice());
    Ok(())
}

#[tokio::test]
async fn key_prefix_isolates_namespaces() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let a = ctx.with_prefix("svc-a").await?;
    let b = ctx.with_prefix("svc-b").await?;

    a.set("shared", &"from a").await?;
    b.set("shared", &"from b").await?;
    assert_eq!(a.get::<String>("shared").await?.as_deref(), Some("from a"));
    assert_eq!(b.get::<String>("shared").await?.as_deref(), Some("from b"));
    assert_eq!(ctx.client.get::<String>("svc-a:shared").await?.as_deref(), Some("from a"));
    assert_eq!(a.prefixed_key("k"), "svc-a:k");

    a.mset(&[("m1", &1), ("m2", &2)]).await?;
    assert_eq!(a.mget::<i32>(&["m1", "m2"]).await?, vec![Some(1), Some(2)]);
    assert_eq!(b.mget::<i32>(&["m1"]).await?, vec![None]);
    Ok(())
}

#[tokio::test]
async fn scan_and_delete_pattern_respect_prefix() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let a = ctx.with_prefix("app[1]*").await?;
    let other = ctx.with_prefix("app[1]x").await?;

    for i in 0..1200 {
        a.set(&format!("session:{i}"), &i).await?;
    }
    a.set("keep", &1).await?;
    other.set("session:1", &1).await?;

    let mut keys = a.scan_keys("session:1*").await?;
    keys.sort();
    assert_eq!(keys.len(), 1 + 10 + 100 + 200);
    assert!(keys.iter().all(|k| k.starts_with("session:1")));

    assert_eq!(a.del_pattern("session:*").await?, 1200);
    assert!(a.exists("keep").await?);
    assert!(other.exists("session:1").await?);
    assert_eq!(a.del_pattern("nothing:*").await?, 0);
    Ok(())
}

#[tokio::test]
async fn hashes() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    client.hset("h", "u1", &alice()).await?;
    client.hmset("h", &[("a", &alice()), ("b", &alice())]).await?;
    assert_eq!(client.hget::<User>("h", "u1").await?, Some(alice()));
    assert_eq!(client.hget::<User>("h", "zz").await?, None);
    assert_eq!(client.hgetall::<User>("h").await?.len(), 3);
    assert!(client.hexists("h", "a").await?);
    assert_eq!(client.hdel("h", &["a", "b", "zz"]).await?, 2);
    assert_eq!(client.hdel("h", &[]).await?, 0);
    assert_eq!(client.hincr("counters", "views", 3).await?, 3);
    Ok(())
}

#[tokio::test]
async fn lists() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    for i in 0..5 {
        client.rpush("l", &i).await?;
    }
    client.lpush("l", &-1).await?;
    assert_eq!(client.lrange::<i32>("l", 0, -1).await?, vec![-1, 0, 1, 2, 3, 4]);
    client.ltrim("l", 0, 2).await?;
    assert_eq!(client.llen("l").await?, 3);
    assert_eq!(client.lpop::<i32>("l").await?, Some(-1));
    assert_eq!(client.rpop::<i32>("l").await?, Some(1));
    assert_eq!(client.lpop::<i32>("empty").await?, None);
    Ok(())
}

#[tokio::test]
async fn sorted_sets() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    assert!(client.zadd("z", "a", 1.0).await?);
    client.zadd("z", "b", 2.0).await?;
    client.zadd("z", "c", 3.0).await?;
    assert!(!client.zadd("z", "a", 1.5).await?);

    assert_eq!(client.zrange("z", 0, -1).await?, vec!["a", "b", "c"]);
    assert_eq!(client.zrevrange("z", 0, 0).await?, vec!["c"]);
    assert_eq!(client.zrangebyscore("z", 1.6, 3.0).await?, vec!["b", "c"]);
    assert_eq!(client.zrange_withscores("z", 0, 0).await?, vec![("a".to_owned(), 1.5)]);
    assert_eq!(client.zscore("z", "b").await?, Some(2.0));
    assert_eq!(client.zincrby("z", "b", 10.0).await?, 12.0);
    assert_eq!(client.zremrangebyscore("z", 0.0, 2.0).await?, 1);
    assert_eq!(client.zrem("z", &["c", "zz"]).await?, 1);
    assert_eq!(client.zcard("z").await?, 1);
    Ok(())
}

#[tokio::test]
async fn sets_and_deletion() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    assert_eq!(client.sadd("s", &["a", "b", "a"]).await?, 2);
    assert!(client.sismember("s", "a").await?);
    assert_eq!(client.scard("s").await?, 2);
    assert_eq!(client.srem("s", &["a"]).await?, 1);
    assert_eq!(client.smembers("s").await?, vec!["b"]);

    client.set("d1", &1).await?;
    client.set("d2", &1).await?;
    assert!(client.del("s").await?);
    assert!(!client.del("s").await?);
    assert_eq!(client.del_many(&["d1", "d2", "nope"]).await?, 2);
    assert_eq!(client.dbsize().await?, 0);
    Ok(())
}

#[tokio::test]
async fn lock_is_exclusive_and_owned() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let client = ctx.with_prefix("jobs").await?;
    let ttl = Duration::from_secs(10);

    let lock = client.try_lock("nightly", ttl).await?.expect("first acquire");
    assert!(client.try_lock("nightly", ttl).await?.is_none());

    assert!(lock.extend(Duration::from_secs(30)).await?);
    let Ttl::Expires(remaining) = client.ttl("nightly").await? else {
        panic!()
    };
    assert!(remaining > ttl);

    assert!(lock.release().await?);
    let again = client.try_lock("nightly", ttl).await?.expect("free after release");
    client.set("nightly", "stolen").await?;
    assert!(!again.extend(ttl).await?);
    assert!(!again.release().await?);
    assert_eq!(client.get::<String>("nightly").await?.as_deref(), Some("stolen"));
    Ok(())
}

#[tokio::test]
async fn lock_expires_and_waits() -> anyhow::Result<()> {
    let Ctx {
        client, container: _c, ..
    } = setup().await?;
    let _held = client.try_lock("l", Duration::from_millis(300)).await?.unwrap();

    let none = client
        .lock(
            "l",
            Duration::from_secs(5),
            Duration::from_millis(100),
            Duration::from_millis(20),
        )
        .await?;
    assert!(none.is_none());

    let acquired = client
        .lock("l", Duration::from_secs(5), Duration::from_secs(2), Duration::from_millis(20))
        .await?;
    assert!(acquired.is_some(), "acquired after the previous holder's TTL expired");
    Ok(())
}

#[tokio::test]
async fn publish_subscribe_with_prefix() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let a = ctx.with_prefix("tenant-a").await?;
    let b = ctx.with_prefix("tenant-b").await?;

    let mut sub = a.subscribe(&["events"]).await?;
    let mut psub = a.psubscribe(&["room:*"]).await?;

    assert_eq!(b.publish("events", &alice()).await?, 0, "other tenant must not be heard");
    assert_eq!(a.publish("events", &alice()).await?, 1);
    a.publish_raw("room:42", b"hello").await?;

    let msg = tokio::time::timeout(Duration::from_secs(5), sub.next_message())
        .await?
        .unwrap();
    assert_eq!(msg.channel, "events");
    assert_eq!(msg.decode::<User>()?, alice());

    let msg = tokio::time::timeout(Duration::from_secs(5), psub.next()).await?.unwrap();
    assert_eq!(msg.channel, "room:42");
    assert_eq!(msg.pattern.as_deref(), Some("room:*"));
    assert_eq!(msg.payload_str(), Some("hello"));
    Ok(())
}

#[tokio::test]
async fn response_timeout_fails_fast() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let client = Valkey::new(ValkeyConfig::new(&ctx.url).response_timeout(Duration::from_millis(200))).await?;

    let started = std::time::Instant::now();
    let err = client
        .query::<Option<(String, String)>>(redis::cmd("BLPOP").arg("never").arg(1))
        .await
        .unwrap_err();
    assert!(err.is_timeout(), "{err}");
    assert!(started.elapsed() < Duration::from_secs(2));

    tokio::time::sleep(Duration::from_millis(1200)).await;
    client.ping().await?;
    Ok(())
}

#[tokio::test]
async fn lazy_connect_does_not_require_server() -> anyhow::Result<()> {
    let client = Valkey::new(
        ValkeyConfig::new("redis://127.0.0.1:1")
            .lazy(true)
            .reconnect(0, Duration::from_millis(10))
            .connection_timeout(Duration::from_millis(200)),
    )
    .await?;
    assert!(client.ping().await.is_err());

    assert!(Valkey::connect("redis://127.0.0.1:1").await.is_err());
    Ok(())
}

#[tokio::test]
async fn raw_commands_pipeline_and_eval() -> anyhow::Result<()> {
    let ctx = setup().await?;
    let client = ctx.with_prefix("p").await?;

    let key = client.prefixed_key("raw");
    let (_, value): ((), String) = client.pipeline(redis::pipe().atomic().set(&key, "v").get(&key)).await?;
    assert_eq!(value, "v");

    let script = redis::Script::new("return redis.call('GET', KEYS[1]) .. ARGV[1]");
    let out: String = client.eval(&script, &["raw"], "!").await?;
    assert_eq!(out, "v!");

    let len: usize = client.query(redis::cmd("STRLEN").arg(&key)).await?;
    assert_eq!(len, 1);
    Ok(())
}
