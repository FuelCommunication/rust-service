pub struct Config {
    pub host: String,
    pub port: String,
    pub origins: String,
    pub scylla_url: String,
    pub scylla_nodes: String,
    pub broadcast_buffer_size: usize,
    pub channels_service_url: String,
    pub scylla_replication_factor: u8,
    pub kafka_brokers: String,
    pub kafka_topic: String,
    pub kafka_group_id: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: read_env_var("HOST"),
            port: read_env_var("PORT"),
            origins: read_env_var("ORIGINS"),
            scylla_url: read_env_var("SCYLLA_URL"),
            scylla_nodes: read_env_var_or("SCYLLA_NODES", ""),
            broadcast_buffer_size: read_env_var_or("BROADCAST_BUFFER_SIZE", "128")
                .parse()
                .expect("BROADCAST_BUFFER_SIZE must be a number"),
            channels_service_url: read_env_var("CHANNELS_SERVICE_URL"),
            scylla_replication_factor: read_env_var_or("SCYLLA_REPLICATION_FACTOR", "1")
                .parse()
                .expect("SCYLLA_REPLICATION_FACTOR must be a number"),
            kafka_brokers: read_env_var("KAFKA_BROKERS"),
            kafka_topic: read_env_var_or("KAFKA_TOPIC", "channels"),
            kafka_group_id: read_env_var_or("KAFKA_GROUP_ID", "service-chats"),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

fn read_env_var_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "0.0.0.0".into(),
            port: "3001".into(),
            origins: "[http://localhost:8080,http://127.0.0.1:8080]".into(),
            scylla_url: "127.0.0.1:9042".into(),
            scylla_nodes: String::new(),
            broadcast_buffer_size: 128,
            channels_service_url: "http://127.0.0.1:8082".into(),
            scylla_replication_factor: 1,
            kafka_brokers: "localhost:9092".into(),
            kafka_topic: "channels".into(),
            kafka_group_id: "service-chats".into(),
        }
    }
}
