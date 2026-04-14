pub struct Config {
    pub listen_addr: String,
    pub images_upstream: String,
    pub chats_upstream: String,
    pub channels_upstream: String,
    pub calls_upstream: String,
    pub auth_upstream: String,
    pub max_req_per_sec: isize,
    pub max_body_size: usize,
    pub connection_timeout_secs: u64,
    pub total_connection_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub write_timeout_secs: u64,
    pub allowed_origins: Vec<String>,
    pub oauth_callback_url: String,
    pub frontend_url: String,
    pub grace_period_secs: u64,
    pub graceful_shutdown_timeout_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen_addr: read_env_var("GATEWAY_LISTEN_ADDR"),
            images_upstream: read_env_var("GATEWAY_IMAGES_UPSTREAM"),
            chats_upstream: read_env_var("GATEWAY_CHATS_UPSTREAM"),
            channels_upstream: read_env_var("GATEWAY_CHANNELS_UPSTREAM"),
            calls_upstream: read_env_var("GATEWAY_CALLS_UPSTREAM"),
            auth_upstream: read_env_var("GATEWAY_AUTH_UPSTREAM"),
            max_req_per_sec: read_env_var("GATEWAY_MAX_REQ_PER_SEC")
                .parse()
                .expect("GATEWAY_MAX_REQ_PER_SEC must be a number"),
            max_body_size: read_env_var("GATEWAY_MAX_BODY_SIZE_MB")
                .parse::<usize>()
                .expect("GATEWAY_MAX_BODY_SIZE_MB must be a number")
                * 1024
                * 1024,
            connection_timeout_secs: read_env_var("GATEWAY_CONN_TIMEOUT_SECS")
                .parse()
                .expect("GATEWAY_CONN_TIMEOUT_SECS must be a number"),
            total_connection_timeout_secs: read_env_var("GATEWAY_TOTAL_CONN_TIMEOUT_SECS")
                .parse()
                .expect("GATEWAY_TOTAL_CONN_TIMEOUT_SECS must be a number"),
            read_timeout_secs: read_env_var("GATEWAY_READ_TIMEOUT_SECS")
                .parse()
                .expect("GATEWAY_READ_TIMEOUT_SECS must be a number"),
            write_timeout_secs: read_env_var("GATEWAY_WRITE_TIMEOUT_SECS")
                .parse()
                .expect("GATEWAY_WRITE_TIMEOUT_SECS must be a number"),
            allowed_origins: parse_list(&std::env::var("GATEWAY_ALLOWED_ORIGINS").unwrap_or_default()),
            oauth_callback_url: std::env::var("GATEWAY_OAUTH_CALLBACK_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/access/oauth/callback".into()),
            frontend_url: std::env::var("GATEWAY_FRONTEND_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
            grace_period_secs: std::env::var("GATEWAY_GRACE_PERIOD_SECS")
                .unwrap_or_else(|_| "5".into())
                .parse()
                .expect("GATEWAY_GRACE_PERIOD_SECS must be a number"),
            graceful_shutdown_timeout_secs: std::env::var("GATEWAY_GRACEFUL_SHUTDOWN_TIMEOUT_SECS")
                .unwrap_or_else(|_| "5".into())
                .parse()
                .expect("GATEWAY_GRACEFUL_SHUTDOWN_TIMEOUT_SECS must be a number"),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

fn parse_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
