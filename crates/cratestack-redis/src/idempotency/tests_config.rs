#![cfg(test)]

use super::config::RedisIdempotencyStoreConfig;
use super::tests_fixtures::offline_store;

#[test]
fn config_trims_outer_colons_and_whitespace() {
    assert_eq!(
        RedisIdempotencyStoreConfig::new(":bank:idem:").key_prefix,
        "bank:idem",
    );
    assert_eq!(
        RedisIdempotencyStoreConfig::new("  bank  ").key_prefix,
        "bank",
    );
}

#[test]
fn config_falls_back_to_default_namespace_when_empty() {
    for input in ["", "::", ":::", "   ", " : : "] {
        assert_eq!(
            RedisIdempotencyStoreConfig::new(input).key_prefix,
            "cratestack",
            "input {input:?} should fall back to default namespace",
        );
    }
}

#[test]
fn config_preserves_inner_colons() {
    // Inner `:` characters are deliberately allowed — Redis ACL
    // hierarchies use them, and stripping them would collapse
    // distinct namespaces like `bank:au:idem` and `bank:nz:idem`.
    assert_eq!(
        RedisIdempotencyStoreConfig::new("bank:au:idem").key_prefix,
        "bank:au:idem",
    );
}

#[test]
fn hash_key_uses_configured_prefix_and_idem_namespace() {
    let store = offline_store("bank");
    let key = store.hash_key("p", "k");
    let suffix = key
        .strip_prefix("bank:idem:")
        .expect("hash_key must use `<prefix>:idem:` as its namespace");
    assert_eq!(suffix.len(), 64);
    assert!(suffix.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn hash_key_is_deterministic() {
    let store = offline_store("bank");
    assert_eq!(store.hash_key("alice", "txn-1"), store.hash_key("alice", "txn-1"));
}

#[test]
fn hash_key_disambiguates_principal_key_boundary() {
    // Naively concatenating `principal || key` would let ("ab","c")
    // and ("a","bc") collide on the same Redis hash, leaking one
    // tenant's response to another. The `0x00` separator (and the
    // SHA-256 wrap) makes that impossible.
    let store = offline_store("bank");
    assert_ne!(store.hash_key("ab", "c"), store.hash_key("a", "bc"));
    assert_ne!(store.hash_key("", "abc"), store.hash_key("abc", ""));
}

#[test]
fn hash_key_isolates_different_prefixes() {
    // Two stores with different prefixes must produce different
    // Redis keys for the same `(principal, key)` — otherwise a
    // staging deployment could overwrite production rows.
    let a = offline_store("staging");
    let b = offline_store("prod");
    assert_ne!(a.hash_key("p", "k"), b.hash_key("p", "k"));
}

#[test]
fn hash_key_handles_pathological_inputs() {
    let store = offline_store("bank");
    // Long principal + key — the SHA-256 wrap keeps the Redis key
    // bounded at exactly 64 hex chars regardless of input size.
    let long_principal = "x".repeat(10_000);
    let long_key = "y".repeat(10_000);
    let result = store.hash_key(&long_principal, &long_key);
    assert!(result.starts_with("bank:idem:"));
    assert_eq!(result.len(), "bank:idem:".len() + 64);
    // Inputs containing `:`, NUL bytes, and other delimiter-like
    // characters must round-trip without breaking the key
    // structure (the SHA-256 makes this trivially true, but a
    // future refactor that drops the hash needs to keep working).
    let weird = store.hash_key("p:rincipal\0", "k\0e:y");
    assert!(weird.starts_with("bank:idem:"));
    assert_eq!(weird.len(), "bank:idem:".len() + 64);
}
