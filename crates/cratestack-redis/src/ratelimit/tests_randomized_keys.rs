#![cfg(test)]

// Randomized property tests for key derivation and prefix normalisation.
//
// These exist to widen coverage past hand-picked inputs. They use a
// tiny xorshift PRNG seeded from `CRATESTACK_TEST_SEED` (or a fixed
// default) so failures are reproducible: re-run with the same env
// var to replay the exact sequence. Every test prints its seed on
// failure via the assertion message.

use super::config::normalize_key_prefix;
use super::tests_fixtures::{XorShift64, offline_store, test_seed};
use super::util::nibble_hex;

#[test]
fn randomized_bucket_key_is_deterministic_and_well_formed() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    let store = offline_store("bank");
    for iteration in 0..200 {
        let key = rng.next_string(64);
        let a = store.bucket_key(&key);
        let b = store.bucket_key(&key);
        assert_eq!(
            a, b,
            "seed={seed:#x} iter={iteration} key={key:?}: bucket_key must be deterministic",
        );
        let suffix = a
            .strip_prefix("bank:rl:")
            .unwrap_or_else(|| panic!("seed={seed:#x} iter={iteration} missing prefix: {a}"));
        assert_eq!(
            suffix.len(),
            64,
            "seed={seed:#x} iter={iteration} key={key:?}: hex suffix must be 64 chars",
        );
        assert!(
            suffix
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "seed={seed:#x} iter={iteration} key={key:?}: suffix {suffix:?} must be lowercase hex",
        );
    }
}

#[test]
fn randomized_bucket_key_collisions_are_negligible() {
    // SHA-256 makes a single collision astronomically unlikely, but
    // a regression that drops or truncates the hash would show up
    // immediately as duplicate keys in a small random sample.
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    let store = offline_store("bank");
    let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for iteration in 0..500 {
        let key = rng.next_string(32);
        let derived = store.bucket_key(&key);
        if let Some(prev) = seen.get(&derived) {
            assert_eq!(
                prev, &key,
                "seed={seed:#x} iter={iteration}: distinct keys {prev:?} and {key:?} mapped to the same bucket {derived}",
            );
        } else {
            seen.insert(derived, key);
        }
    }
}

#[test]
fn randomized_normalize_key_prefix_never_emits_outer_colon_or_whitespace() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..200 {
        let raw = rng.next_string(40);
        let normalized = normalize_key_prefix(raw.clone());
        // The fallback default is always non-empty.
        assert!(
            !normalized.is_empty(),
            "seed={seed:#x} iter={iteration} raw={raw:?}: must never be empty",
        );
        // No leading/trailing whitespace or `:` after normalisation.
        assert!(
            !normalized.starts_with(':')
                && !normalized.ends_with(':')
                && !normalized.starts_with(char::is_whitespace)
                && !normalized.ends_with(char::is_whitespace),
            "seed={seed:#x} iter={iteration} raw={raw:?} normalized={normalized:?}: outer noise must be stripped",
        );
    }
}

#[test]
fn randomized_nibble_hex_only_emits_lowercase_hex_chars() {
    let seed = test_seed();
    let mut rng = XorShift64::new(seed);
    for iteration in 0..200 {
        let byte = (rng.next_u32() & 0x0f) as u8;
        let ch = nibble_hex(byte);
        assert!(
            ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase(),
            "seed={seed:#x} iter={iteration} byte={byte:#x} -> {ch}",
        );
    }
}
