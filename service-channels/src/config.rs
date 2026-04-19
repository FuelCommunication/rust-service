pub struct Config {
    pub host: String,
    pub port: String,
    pub origins: String,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    pub valkey_url: String,
    pub cache_ttl_secs: u64,
    pub meilisearch_url: String,
    pub meilisearch_api_key: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: read_env_var("HOST"),
            port: read_env_var("PORT"),
            origins: read_env_var("ORIGINS"),
            database_url: read_env_var("DATABASE_URL"),
            db_max_connections: read_env_var_or("DB_MAX_CONNECTIONS", "10")
                .parse()
                .expect("DB_MAX_CONNECTIONS must be a number"),
            db_min_connections: read_env_var_or("DB_MIN_CONNECTIONS", "1")
                .parse()
                .expect("DB_MIN_CONNECTIONS must be a number"),
            db_acquire_timeout_secs: read_env_var_or("DB_ACQUIRE_TIMEOUT_SECS", "5")
                .parse()
                .expect("DB_ACQUIRE_TIMEOUT_SECS must be a number"),
            valkey_url: read_env_var("VALKEY_URL"),
            cache_ttl_secs: read_env_var_or("CACHE_TTL_SECS", "300")
                .parse()
                .expect("CACHE_TTL_SECS must be a number"),
            meilisearch_url: read_env_var("MEILISEARCH_URL"),
            meilisearch_api_key: std::env::var("MEILISEARCH_API_KEY").ok().filter(|s| !s.is_empty()),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

fn read_env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
