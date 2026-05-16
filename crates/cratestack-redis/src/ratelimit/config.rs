#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisRateLimitStoreConfig {
    pub key_prefix: String,
}

impl RedisRateLimitStoreConfig {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: normalize_key_prefix(key_prefix.into()),
        }
    }
}

pub(super) fn normalize_key_prefix(key_prefix: String) -> String {
    let cleaned = key_prefix.trim().trim_matches(':').trim();
    if cleaned.is_empty() {
        "cratestack".to_owned()
    } else {
        cleaned.to_owned()
    }
}
