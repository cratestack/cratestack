//! An HMAC key verifies exactly the algorithm it is configured for
//! (security review of cratestack#1005, fix 2).
//!
//! HMAC 256/64 and 256/256 use the same secret the same way and differ
//! only in how much of the tag is sent. Before the fix, a key accepted
//! both, so a deployment that chose 256/256 would also accept a 64-bit tag
//! from any sender that asked for alg 4: a forger's work fell from 2²⁵⁶ to
//! 2⁶⁴ online attempts, and nothing on the verifier could refuse it.

mod common;

use std::sync::Arc;

use common::rpc_request;
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::{CoseAlg, CoseVerifyKey, StaticVerifierResolver, UNAUTHENTICATED};

fn is_coarse_401<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

/// A server whose resolver holds the shared secret for `algs` only.
fn server_accepting(algs: &[CoseAlg], now: u64) -> cratestack_cose::CoseEnvelope {
    let resolver = algs
        .iter()
        .fold(StaticVerifierResolver::new(), |resolver, &alg| {
            resolver.with_key(common::hmac(alg).verify_key())
        });
    common::server_with(
        CoseAlg::Hmac256_256,
        now,
        Arc::new(resolver),
        Arc::new(InMemoryNonceStore::new()),
    )
}

#[tokio::test]
async fn a_256_256_key_never_accepts_a_64_bit_tag() {
    let now = common::now();
    let sealed = common::sealed_request_at(CoseAlg::Hmac256_64, &rpc_request(), now).await;
    let result = server_accepting(&[CoseAlg::Hmac256_256], now)
        .open_request(sealed.clone(), &rpc_request())
        .await;
    assert!(
        is_coarse_401(&result),
        "256/256 key accepted alg 4: {result:?}"
    );
    // Control: a deployment that lists the secret for 256/64 accepts it.
    server_accepting(&[CoseAlg::Hmac256_64], now)
        .open_request(sealed, &rpc_request())
        .await
        .expect("the 256/64 key verifies its own algorithm");
}

#[tokio::test]
async fn a_256_64_key_never_accepts_a_256_bit_tag() {
    let now = common::now();
    let sealed = common::sealed_request_at(CoseAlg::Hmac256_256, &rpc_request(), now).await;
    let result = server_accepting(&[CoseAlg::Hmac256_64], now)
        .open_request(sealed, &rpc_request())
        .await;
    assert!(is_coarse_401(&result), "{result:?}");
}

/// Listing the secret for both algorithms accepts both, each through its
/// own key. (Distinct `cti`s: the two keys share the secret's `kid`, so the
/// same `cti` under both would be one `(kid, cti)` seen twice, a replay.)
#[tokio::test]
async fn one_secret_listed_for_both_algorithms_accepts_both() {
    let now = common::now();
    let server = server_accepting(&[CoseAlg::Hmac256_64, CoseAlg::Hmac256_256], now);
    for (alg, cti) in [(CoseAlg::Hmac256_64, "01"), (CoseAlg::Hmac256_256, "02")] {
        let sealed = common::client(alg, now, cti)
            .seal_request(&common::fixture::payment_bytes(), &rpc_request())
            .await
            .expect("seal");
        let opened = server
            .open_request(sealed, &rpc_request())
            .await
            .expect("opens");
        assert_eq!(opened.alg, alg);
    }
}

#[test]
fn a_key_supports_exactly_its_own_algorithm() {
    for &key_alg in &[CoseAlg::Hmac256_64, CoseAlg::Hmac256_256] {
        let key = CoseVerifyKey::hmac(key_alg, common::HMAC_SECRET.to_vec()).expect("key");
        assert_eq!(key.alg(), key_alg);
        for &alg in CoseAlg::ALL {
            assert_eq!(key.supports(alg), alg == key_alg, "{key_alg:?} vs {alg:?}");
        }
    }
    let ed = common::ed25519().verify_key();
    assert!(ed.supports(CoseAlg::Ed25519) && !ed.supports(CoseAlg::Esp256));
    let p = common::p256().verify_key();
    assert!(p.supports(CoseAlg::Esp256) && !p.supports(CoseAlg::Ed25519));
}
