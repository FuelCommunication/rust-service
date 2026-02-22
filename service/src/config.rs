pub struct Config {
    pub host: String,
    pub port: String,
    pub origins: String,
    pub s3: S3Config,
    pub kafka: KafkaConfig,
    pub scylla_url: String,
}

pub struct S3Config {
    pub access_key: String,
    pub secret_key: String,
    pub region: String,
    pub endpoint_url: String,
    pub bucket: String,
}

pub struct KafkaConfig {
    pub brokers: String,
    pub topic: String,
    pub group_id: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            host: read_env_var("HOST"),
            port: read_env_var("PORT"),
            origins: read_env_var("ORIGINS"),
            s3: S3Config {
                access_key: read_env_var("ACCESS_KEY"),
                secret_key: read_env_var("SECRET_KEY"),
                region: read_env_var("REGION"),
                endpoint_url: read_env_var("ENDPOINT_URL"),
                bucket: read_env_var("BUCKET"),
            },
            kafka: KafkaConfig {
                brokers: read_env_var("BROKERS"),
                topic: read_env_var("TOPIC"),
                group_id: read_env_var("GROUP_ID"),
            },
            scylla_url: read_env_var("SCYLLA_URL"),
        }
    }
}

fn read_env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Required environment variable {key} is not set"))
}

impl Default for Config {
    fn default() -> Self {
        Config {
            host: "0.0.0.0".into(),
            port: "3000".into(),
            origins: "[http://localhost:8080,http://127.0.0.1:8080]".into(),
            s3: S3Config {
                access_key: "admin".into(),
                secret_key: "admin12345".into(),
                region: "us-east-1".into(),
                endpoint_url: "http://localhost:9000".into(),
                bucket: "my-bucket".into(),
            },
            kafka: KafkaConfig {
                brokers: "127.0.0.1:9092".into(),
                topic: "images".into(),
                group_id: "rust-service".into(),
            },
            scylla_url: "127.0.0.1:9042".into(),
        }
    }
}
