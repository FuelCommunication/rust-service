const MIN_JWT_SECRET_LENGTH: usize = 32;

pub struct Config {
    pub grpc_port: u16,
    pub database_url: String,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    pub jwt_secret: String,
    pub jwt_access_expiration_secs: u64,
    pub jwt_refresh_expiration_secs: u64,
    pub google_client_id: Option<String>,
    pub google_client_secret: Option<String>,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub allowed_redirect_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let jwt_secret = read_env_var("JWT_SECRET");
        assert!(
            jwt_secret.len() >= MIN_JWT_SECRET_LENGTH,
            "JWT_SECRET must be at least {MIN_JWT_SECRET_LENGTH} characters"
        );

        Self {
            grpc_port: read_env_var("GRPC_PORT")
                .parse()
                .expect("GRPC_PORT must be a valid port number"),
            database_url: read_env_var("DATABASE_URL"),
            db_max_connections: read_env_var_or("DB_MAX_CONNECTIONS", "10")
                .parse()
                .expect("DB_MAX_CONNECTIONS must be a number"),
            db_min_connections: read_env_var_or("DB_MIN_CONNECTIONS", "2")
                .parse()
                .expect("DB_MIN_CONNECTIONS must be a number"),
            db_acquire_timeout_secs: read_env_var_or("DB_ACQUIRE_TIMEOUT_SECS", "5")
                .parse()
                .expect("DB_ACQUIRE_TIMEOUT_SECS must be a number"),
            jwt_secret,
            jwt_access_expiration_secs: read_env_var("JWT_ACCESS_EXPIRATION")
                .parse()
                .expect("JWT_ACCESS_EXPIRATION must be a number (seconds)"),
            jwt_refresh_expiration_secs: read_env_var("JWT_REFRESH_EXPIRATION")
                .parse()
                .expect("JWT_REFRESH_EXPIRATION must be a number (seconds)"),
            google_client_id: read_optional_env_var("GOOGLE_CLIENT_ID"),
            google_client_secret: read_optional_env_var("GOOGLE_CLIENT_SECRET"),
            github_client_id: read_optional_env_var("GITHUB_CLIENT_ID"),
            github_client_secret: read_optional_env_var("GITHUB_CLIENT_SECRET"),
            allowed_redirect_origins: read_env_var_or("ALLOWED_REDIRECT_ORIGINS", "")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

fn read_env_var_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn read_optional_env_var(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}
