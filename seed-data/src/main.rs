use anyhow::{Context, Result};
use argon2::password_hash::{SaltString, rand_core::OsRng};
use argon2::{Argon2, PasswordHasher};
use chrono::{Duration, Utc};
use clap::Parser;
use fake::Fake;
use fake::faker::internet::en::{SafeEmail, Username};
use fake::faker::lorem::en::{Paragraph, Sentence, Word};
use fake::faker::name::en::Name;
use rand::seq::{IndexedRandom, SliceRandom};
use rand::{Rng, rng};
use scylladb_client::{ChatMessageStore, ScyllaConfig};
use sqlx::PgPool;
use std::time::Duration as StdDuration;
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(about = "Generate fake data for FuelCommunication databases")]
struct Args {
    #[arg(long, env = "AUTH_DATABASE_URL", default_value = "postgres://postgres:postgres@localhost:5432/fuel_auth")]
    auth_db: String,

    #[arg(long, env = "CHANNELS_DATABASE_URL", default_value = "postgres://postgres:postgres@localhost:5433/fuel_channels")]
    channels_db: String,

    #[arg(long, env = "SCYLLA_URI", default_value = "127.0.0.1:9042")]
    scylla_uri: String,

    #[arg(long, env = "SCYLLA_KEYSPACE", default_value = "chat")]
    scylla_keyspace: String,

    #[arg(long, env = "SCYLLA_REPLICATION", default_value_t = 1)]
    scylla_replication: u8,

    #[arg(long, default_value_t = 50)]
    users: usize,

    #[arg(long, default_value_t = 15)]
    channels: usize,

    #[arg(long, default_value_t = 10)]
    avg_subscribers_per_channel: usize,

    #[arg(long, default_value_t = 30)]
    avg_messages_per_channel: usize,

    #[arg(long, default_value = "password123")]
    password: String,

    #[arg(long, help = "Wipe target tables before seeding")]
    truncate: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt().with_env_filter("info,seed_data=debug").init();

    let args = Args::parse();

    tracing::info!("Connecting to auth db");
    let auth_pool = PgPool::connect(&args.auth_db).await.context("auth db")?;

    tracing::info!("Connecting to channels db");
    let channels_pool = PgPool::connect(&args.channels_db).await.context("channels db")?;

    tracing::info!("Connecting to ScyllaDB at {}", args.scylla_uri);
    let scylla_config = ScyllaConfig {
        uri: args.scylla_uri.clone(),
        additional_nodes: vec![],
        connection_timeout: StdDuration::from_secs(5),
        metadata_refresh_interval: StdDuration::from_secs(10),
        keyspace: args.scylla_keyspace.clone(),
        replication_factor: args.scylla_replication,
    };
    let chat_store = ChatMessageStore::new(&scylla_config, true).await.context("scylla connect")?;

    if args.truncate {
        truncate(&auth_pool, &channels_pool, &chat_store, &args.scylla_keyspace).await?;
    }

    let users = seed_users(&auth_pool, args.users, &args.password).await?;
    let channels = seed_channels(&channels_pool, args.channels, &users, args.avg_subscribers_per_channel).await?;
    seed_messages(&chat_store, &channels, &users, args.avg_messages_per_channel).await?;

    tracing::info!(
        users = users.len(),
        channels = channels.len(),
        "seed complete — login: alice@example.com / {}",
        args.password
    );

    Ok(())
}

async fn truncate(
    auth: &PgPool,
    channels: &PgPool,
    chat: &ChatMessageStore,
    keyspace: &str,
) -> Result<()> {
    tracing::warn!("truncating all tables");
    sqlx::query("TRUNCATE oauth_accounts, refresh_tokens, users CASCADE").execute(auth).await?;
    sqlx::query("TRUNCATE channel_subscribers, channels CASCADE").execute(channels).await?;
    let session = chat.session();
    for table in ["messages", "user_messages", "message_by_id"] {
        session.query_unpaged(format!("TRUNCATE {keyspace}.{table}"), &[]).await?;
    }
    Ok(())
}

async fn seed_users(pool: &PgPool, count: usize, password: &str) -> Result<Vec<Uuid>> {
    tracing::info!("seeding {count} users");
    let hash = hash_password(password)?;

    let alice_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users (id, email, username, password_hash) VALUES ($1, $2, $3, $4) \
         ON CONFLICT (email) DO UPDATE SET password_hash = EXCLUDED.password_hash RETURNING id",
    )
    .bind(alice_id)
    .bind("alice@example.com")
    .bind("alice")
    .bind(&hash)
    .execute(pool)
    .await?;

    let mut ids = vec![alice_id];

    for i in 0..count.saturating_sub(1) {
        let id = Uuid::new_v4();
        let email: String = SafeEmail().fake();
        let username: String = format!("{}_{}", Username().fake::<String>(), i);
        let bio: String = Sentence(5..15).fake();

        let row: Result<(Uuid,), _> = sqlx::query_as(
            "INSERT INTO users (id, email, username, password_hash, bio) VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(id)
        .bind(&email)
        .bind(&username)
        .bind(&hash)
        .bind(&bio)
        .fetch_one(pool)
        .await;

        if let Ok((id,)) = row {
            ids.push(id);
        }
    }

    tracing::info!("inserted {} users", ids.len());
    Ok(ids)
}

async fn seed_channels(
    pool: &PgPool,
    count: usize,
    user_ids: &[Uuid],
    avg_subs: usize,
) -> Result<Vec<Uuid>> {
    tracing::info!("seeding {count} channels");
    let mut rng = rng();
    let mut channel_ids = Vec::with_capacity(count);

    for i in 0..count {
        let id = Uuid::new_v4();
        let title = format!("{}-{i}", Word().fake::<String>());
        let description: String = Sentence(8..20).fake();

        let row: Result<(Uuid,), _> = sqlx::query_as(
            "INSERT INTO channels (id, title, description) VALUES ($1, $2, $3) \
             ON CONFLICT DO NOTHING RETURNING id",
        )
        .bind(id)
        .bind(&title)
        .bind(&description)
        .fetch_one(pool)
        .await;

        let Ok((channel_id,)) = row else { continue };
        channel_ids.push(channel_id);

        let owner = user_ids.choose(&mut rng).copied().unwrap();
        sqlx::query(
            "INSERT INTO channel_subscribers (id, user_id, channel_id, is_owner) \
             VALUES ($1, $2, $3, true) ON CONFLICT DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(owner)
        .bind(channel_id)
        .execute(pool)
        .await?;

        let extra: usize = rng.random_range(avg_subs.saturating_sub(3)..=avg_subs + 3);
        let mut pool_users: Vec<Uuid> = user_ids.iter().filter(|&&u| u != owner).copied().collect();
        pool_users.shuffle(&mut rng);

        for sub_user in pool_users.iter().take(extra) {
            sqlx::query(
                "INSERT INTO channel_subscribers (id, user_id, channel_id, is_owner) \
                 VALUES ($1, $2, $3, false) ON CONFLICT DO NOTHING",
            )
            .bind(Uuid::new_v4())
            .bind(sub_user)
            .bind(channel_id)
            .execute(pool)
            .await?;
        }
    }

    tracing::info!("inserted {} channels", channel_ids.len());
    Ok(channel_ids)
}

async fn seed_messages(
    chat: &ChatMessageStore,
    channel_ids: &[Uuid],
    user_ids: &[Uuid],
    avg: usize,
) -> Result<()> {
    tracing::info!("seeding messages (~{avg} per channel)");
    let mut rng = rng();
    let mut total = 0usize;

    for &channel_id in channel_ids {
        let n: usize = rng.random_range(avg.saturating_sub(5)..=avg + 5);
        for _ in 0..n {
            let user = user_ids.choose(&mut rng).copied().unwrap();
            let content: String = Paragraph(1..3).fake();
            chat.create_message(channel_id, user, content).await?;
            total += 1;
        }
    }

    tracing::info!("inserted {total} messages");
    Ok(())
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("argon2: {e}"))
}

#[allow(dead_code)]
fn random_recent() -> chrono::DateTime<Utc> {
    let mut rng = rng();
    let days_ago: i64 = rng.random_range(0..30);
    let secs: i64 = rng.random_range(0..86_400);
    Utc::now() - Duration::days(days_ago) - Duration::seconds(secs)
}

#[allow(dead_code)]
fn random_name() -> String {
    Name().fake()
}
