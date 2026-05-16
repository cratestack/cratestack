#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisIdempotencyStoreConfig {
    pub key_prefix: String,
}

impl RedisIdempotencyStoreConfig {
    pub fn new(key_prefix: impl Into<String>) -> Self {
        Self {
            key_prefix: normalize_key_prefix(key_prefix.into()),
        }
    }
}

pub(super) fn normalize_key_prefix(key_prefix: String) -> String {
    // Trim in the order outer-whitespace → outer-colons → outer-whitespace
    // so inputs like `" : : "` (any mix of leading/trailing whitespace and
    // colon delimiters) reduce to empty and fall back to the default.
    // A previous order of `trim_matches(':').trim()` left whitespace-
    // wrapped colon noise in the prefix.
    let cleaned = key_prefix.trim().trim_matches(':').trim();
    if cleaned.is_empty() {
        "cratestack".to_owned()
    } else {
        cleaned.to_owned()
    }
}
