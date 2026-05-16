#![cfg(test)]

use super::config::RedisRateLimitStoreConfig;
use super::tests_fixtures::offline_store;

#[test]
fn config_trims_outer_colons_and_whitespace() {
    assert_eq!(
        RedisRateLimitStoreConfig::new(":bank:rl:").key_prefix,
        "bank:rl",
    );
    assert_eq!(
        RedisRateLimitStoreConfig::new("  bank  ").key_prefix,
        "bank",
    );
}

#[test]
fn config_falls_back_to_default_namespace_when_empty() {
    for input in ["", "::", ":::", "   ", " : : "] {
        assert_eq!(
            RedisRateLimitStoreConfig::new(input).key_prefix,
            "cratestack",
            "input {input:?} should fall back to default namespace",
        );
    }
}

#[test]
fn bucket_key_uses_configured_prefix_and_rl_namespace() {
    let store = offline_store("bank");
    let key = store.bucket_key("auth:abc");
    let suffix = key
        .strip_prefix("bank:rl:")
        .expect("bucket_key must use `<prefix>:rl:` as its namespace");
    assert_eq!(suffix.len(), 64);
    assert!(suffix
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn bucket_key_is_deterministic_and_distinct_per_input() {
    let store = offline_store("bank");
    assert_eq!(store.bucket_key("alice"), store.bucket_key("alice"));
    assert_ne!(store.bucket_key("alice"), store.bucket_key("bob"));
}

#[test]
fn bucket_key_isolates_different_prefixes() {
    let a = offline_store("staging");
    let b = offline_store("prod");
    assert_ne!(a.bucket_key("k"), b.bucket_key("k"));
}

#[test]
fn bucket_key_handles_pathological_inputs() {
    let store = offline_store("bank");
    let long = "x".repeat(10_000);
    let result = store.bucket_key(&long);
    assert!(result.starts_with("bank:rl:"));
    assert_eq!(result.len(), "bank:rl:".len() + 64);
}
