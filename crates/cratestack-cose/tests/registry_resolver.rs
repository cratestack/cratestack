//! `RegistryVerifierResolver`: the `CoseVerifierResolver` contract of
//! `static_resolver.rs`, plus run-time registration, revocation, the
//! capacity bound, and concurrent use. The envelope-level behaviour is
//! `registry_envelope.rs`.

mod common;

use std::sync::Arc;

use cratestack_cose::{CoseAlg, CoseVerifierResolver, RegistryVerifierResolver};

fn hmac_keys() -> [cratestack_cose::CoseVerifyKey; 2] {
    [CoseAlg::Hmac256_64, CoseAlg::Hmac256_256].map(|alg| common::hmac(alg).verify_key())
}

#[tokio::test]
async fn every_algorithm_resolves_under_its_own_kid_and_alg_only() {
    let registry = RegistryVerifierResolver::new();
    let mut all = vec![common::ed25519().verify_key(), common::p256().verify_key()];
    all.extend(hmac_keys());
    for key in &all {
        assert_eq!(registry.register(key.clone()).expect("register"), key.kid());
    }
    assert_eq!(registry.len(), 4);
    for key in &all {
        for &alg in CoseAlg::ALL {
            let found = registry.resolve(&key.kid(), alg).await.expect("resolve");
            // The two HMAC keys share a kid, so the alg picks between them.
            let expected: Vec<_> = all
                .iter()
                .filter(|held| held.kid() == key.kid() && held.alg() == alg)
                .cloned()
                .collect();
            assert_eq!(found, expected, "{:?} asked as {alg:?}", key.alg());
        }
    }
}

#[tokio::test]
async fn unknown_or_malformed_kids_are_an_empty_answer() {
    let registry = RegistryVerifierResolver::new();
    let key = common::ed25519().verify_key();
    registry.register(key.clone()).expect("register");
    let kid = key.kid();
    for asked in [
        &[0; 8][..],
        &kid[..7],
        &[kid.as_slice(), &[0]].concat(),
        &[],
    ] {
        assert!(
            registry
                .resolve(asked, CoseAlg::Ed25519)
                .await
                .expect("resolve")
                .is_empty()
        );
    }
}

#[tokio::test]
async fn registering_twice_is_idempotent() {
    let registry = RegistryVerifierResolver::new();
    let key = common::p256().verify_key();
    let first = registry.register(key.clone()).expect("register");
    assert_eq!(registry.register(key.clone()).expect("again"), first);
    assert_eq!(registry.len(), 1);
    let found = registry
        .resolve(&first, CoseAlg::Esp256)
        .await
        .expect("resolve");
    assert_eq!(found, vec![key]);
}

#[tokio::test]
async fn revoke_removes_by_kid_and_is_repeatable() {
    let registry = RegistryVerifierResolver::new();
    let key = common::ed25519().verify_key();
    let other = common::p256().verify_key();
    registry.register(key.clone()).expect("register");
    registry.register(other.clone()).expect("register");
    assert_eq!(registry.revoke(&key.kid()), 1);
    assert_eq!(registry.revoke(&key.kid()), 0);
    assert_eq!(registry.revoke(&key.kid()[..3]), 0);
    assert!(
        registry
            .resolve(&key.kid(), CoseAlg::Ed25519)
            .await
            .expect("r")
            .is_empty()
    );
    assert_eq!(
        registry
            .resolve(&other.kid(), CoseAlg::Esp256)
            .await
            .expect("r"),
        vec![other]
    );
    assert_eq!(registry.len(), 1);
    // A revoked key can be registered again.
    registry.register(key.clone()).expect("re-register");
    assert_eq!(registry.len(), 2);
}

#[tokio::test]
async fn one_hmac_secret_for_both_algorithms_shares_a_kid() {
    let registry = RegistryVerifierResolver::new();
    let [short, full] = hmac_keys();
    registry.register(short.clone()).expect("register");
    registry.register(full.clone()).expect("register");
    assert_eq!(short.kid(), full.kid());
    assert_eq!(registry.len(), 2);

    assert!(registry.revoke_key(&short));
    assert!(!registry.revoke_key(&short));
    let kid = full.kid();
    assert!(
        registry
            .resolve(&kid, CoseAlg::Hmac256_64)
            .await
            .expect("r")
            .is_empty()
    );
    assert_eq!(
        registry
            .resolve(&kid, CoseAlg::Hmac256_256)
            .await
            .expect("r"),
        vec![full]
    );

    registry.register(short).expect("register");
    assert_eq!(registry.revoke(&kid), 2, "revoke(kid) takes both");
    assert!(registry.is_empty());
}

#[test]
fn max_keys_bounds_new_keys_but_not_repeats() {
    let registry = RegistryVerifierResolver::with_max_keys(1);
    let key = common::ed25519().verify_key();
    registry.register(key.clone()).expect("fits");
    registry
        .register(key.clone())
        .expect("a repeat is not a new key");
    let error = registry
        .register(common::p256().verify_key())
        .expect_err("full");
    assert!(
        matches!(error, cratestack_core::CratestackError::Conflict(_)),
        "{error:?}"
    );
    assert_eq!(registry.len(), 1);
    // Revoking frees the slot.
    registry.revoke(&key.kid());
    registry
        .register(common::p256().verify_key())
        .expect("room again");
    assert!(
        RegistryVerifierResolver::with_max_keys(0)
            .register(key)
            .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_register_revoke_and_resolve_stay_consistent() {
    let registry = Arc::new(RegistryVerifierResolver::new());
    let key = common::ed25519().verify_key();
    let kid = key.kid();
    let stable = common::p256().verify_key();
    registry.register(stable.clone()).expect("register");

    let mut tasks = Vec::new();
    for _ in 0..4 {
        let (writer, churned) = (registry.clone(), key.clone());
        tasks.push(tokio::spawn(async move {
            for _ in 0..200 {
                writer.register(churned.clone()).expect("register");
                writer.revoke(&churned.kid());
            }
        }));
        let (reader, key, stable) = (registry.clone(), key.clone(), stable.clone());
        tasks.push(tokio::spawn(async move {
            for _ in 0..200 {
                // Never torn: absent, or exactly the one key; and an
                // unrelated key is unaffected throughout.
                let seen = reader
                    .resolve(&key.kid(), CoseAlg::Ed25519)
                    .await
                    .expect("r");
                assert!(seen.is_empty() || seen == vec![key.clone()], "{seen:?}");
                let kept = reader
                    .resolve(&stable.kid(), CoseAlg::Esp256)
                    .await
                    .expect("r");
                assert_eq!(kept, vec![stable.clone()]);
            }
        }));
    }
    for task in tasks {
        task.await.expect("task");
    }
    registry.revoke(&kid);
    assert_eq!(registry.len(), 1, "the bookkeeping survived the churn");
}
