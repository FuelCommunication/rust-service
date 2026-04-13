pub struct Config {
    pub host: String,
    pub port: String,
    pub origins: String,
    pub max_peers_per_room: usize,
    pub room_idle_timeout_secs: u64,
    pub max_message_size: usize,
    pub channel_capacity: usize,
    pub request_timeout_secs: u64,
    pub heartbeat_interval_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: read_env_var("HOST"),
            port: read_env_var("PORT"),
            origins: read_env_var("ORIGINS"),
            max_peers_per_room: read_env_var_or("MAX_PEERS_PER_ROOM", "4")
                .parse()
                .expect("MAX_PEERS_PER_ROOM must be a number"),
            room_idle_timeout_secs: read_env_var_or("ROOM_IDLE_TIMEOUT_SECS", "30")
                .parse()
                .expect("ROOM_IDLE_TIMEOUT_SECS must be a number"),
            max_message_size: read_env_var_or("MAX_MESSAGE_SIZE", "65536")
                .parse()
                .expect("MAX_MESSAGE_SIZE must be a number"),
            channel_capacity: read_env_var_or("CHANNEL_CAPACITY", "64")
                .parse()
                .expect("CHANNEL_CAPACITY must be a number"),
            request_timeout_secs: read_env_var_or("REQUEST_TIMEOUT_SECS", "10")
                .parse()
                .expect("REQUEST_TIMEOUT_SECS must be a number"),
            heartbeat_interval_secs: read_env_var_or("HEARTBEAT_INTERVAL_SECS", "15")
                .parse()
                .expect("HEARTBEAT_INTERVAL_SECS must be a number"),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

fn read_env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
