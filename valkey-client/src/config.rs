use std::time::Duration;

#[derive(Clone)]
pub struct ValkeyConfig {
    pub(crate) url: String,
    pub(crate) key_prefix: Option<String>,
    pub(crate) connection_timeout: Duration,
    pub(crate) response_timeout: Duration,
    pub(crate) reconnect_retries: usize,
    pub(crate) reconnect_max_delay: Duration,
    pub(crate) lazy: bool,
}

impl ValkeyConfig {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            key_prefix: None,
            connection_timeout: Duration::from_secs(5),
            response_timeout: Duration::from_secs(2),
            reconnect_retries: 6,
            reconnect_max_delay: Duration::from_secs(2),
            lazy: false,
        }
    }

    pub fn key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = Some(prefix.into()).filter(|p| !p.is_empty());
        self
    }

    pub fn connection_timeout(mut self, timeout: Duration) -> Self {
        self.connection_timeout = timeout;
        self
    }

    pub fn response_timeout(mut self, timeout: Duration) -> Self {
        self.response_timeout = timeout;
        self
    }

    pub fn reconnect(mut self, retries: usize, max_delay: Duration) -> Self {
        self.reconnect_retries = retries;
        self.reconnect_max_delay = max_delay;
        self
    }

    pub fn lazy(mut self, lazy: bool) -> Self {
        self.lazy = lazy;
        self
    }
}

impl std::fmt::Debug for ValkeyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValkeyConfig")
            .field("url", &redact_url(&self.url))
            .field("key_prefix", &self.key_prefix)
            .field("connection_timeout", &self.connection_timeout)
            .field("response_timeout", &self.response_timeout)
            .field("reconnect_retries", &self.reconnect_retries)
            .field("reconnect_max_delay", &self.reconnect_max_delay)
            .field("lazy", &self.lazy)
            .finish()
    }
}

pub(crate) fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    match rest.rsplit_once('@') {
        Some((credentials, host)) => {
            let user = credentials.split_once(':').map_or(credentials, |(user, _)| user);
            format!("{scheme}://{user}:***@{host}")
        }
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_password() {
        assert_eq!(redact_url("redis://:secret@host:6379/0"), "redis://:***@host:6379/0");
        assert_eq!(redact_url("rediss://user:secret@host"), "rediss://user:***@host");
        assert_eq!(redact_url("redis://host:6379"), "redis://host:6379");
        assert!(!format!("{:?}", ValkeyConfig::new("redis://:secret@h")).contains("secret"));
    }

    #[test]
    fn empty_prefix_is_ignored() {
        assert_eq!(ValkeyConfig::new("redis://h").key_prefix("").key_prefix, None);
    }
}
