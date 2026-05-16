//! Configuration for the Redis-backed client state store.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisStateStoreConfig {
    pub key_prefix: String,
}

impl RedisStateStoreConfig {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: normalize_key_prefix(key_prefix.into()),
        }
    }

    pub(crate) fn meta_key(&self) -> String {
        format!("{}:meta", self.key_prefix)
    }

    pub(crate) fn request_journal_key(&self) -> String {
        format!("{}:request_journal", self.key_prefix)
    }
}

fn normalize_key_prefix(key_prefix: String) -> String {
    let key_prefix = key_prefix.trim_matches(':').trim().to_owned();
    if key_prefix.is_empty() {
        "cratestack:client".to_owned()
    } else {
        key_prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_normalizes_keys() {
        let config = RedisStateStoreConfig::new(":example:orders:");

        assert_eq!(config.key_prefix, "example:orders");
        assert_eq!(config.meta_key(), "example:orders:meta");
        assert_eq!(
            config.request_journal_key(),
            "example:orders:request_journal"
        );
    }
}
