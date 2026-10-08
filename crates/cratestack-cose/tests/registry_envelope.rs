//! A `RegistryVerifierResolver` behind a real server envelope: a key
//! registered at run time is accepted, a revoked or never-registered one is
//! the coarse `401`, for every algorithm. Each request carries its own
//! `cti`, so a refusal is the key's and never a replay.

mod common;

use std::sync::Arc;

use common::{CTI_2, CTI_16, rest_request};
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::{CoseAlg, RegistryVerifierResolver, UNAUTHENTICATED};

const CTIS: [&str; 3] = [CTI_16, CTI_2, "aabbccddeeff00112233445566778899"];

fn verify_key(alg: CoseAlg) -> cratestack_cose::CoseVerifyKey {
    match alg {
        CoseAlg::Ed25519 => common::ed25519().verify_key(),
        CoseAlg::Esp256 => common::p256().verify_key(),
        _ => common::hmac(alg).verify_key(),
    }
}

async fn open(
    registry: &Arc<RegistryVerifierResolver>,
    alg: CoseAlg,
    cti: &str,
) -> Result<(), CratestackError> {
    let now = common::now();
    let sealed = common::client(alg, now, cti)
        .seal_request(&common::fixture::payment_bytes(), &rest_request())
        .await
        .expect("seal");
    common::server_with(
        alg,
        now,
        registry.clone(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .open_request(sealed, &rest_request())
    .await
    .map(|_| ())
}

fn is_coarse_401(result: &Result<(), CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

#[tokio::test]
async fn register_then_revoke_gates_every_algorithm() {
    for &alg in CoseAlg::ALL {
        let registry = Arc::new(RegistryVerifierResolver::new());
        let key = verify_key(alg);

        let unknown = open(&registry, alg, CTIS[0]).await;
        assert!(is_coarse_401(&unknown), "{alg:?} unknown: {unknown:?}");

        registry.register(key.clone()).expect("register");
        open(&registry, alg, CTIS[1])
            .await
            .unwrap_or_else(|e| panic!("{alg:?}: {e:?}"));

        assert!(registry.revoke_key(&key));
        let revoked = open(&registry, alg, CTIS[2]).await;
        assert!(is_coarse_401(&revoked), "{alg:?} revoked: {revoked:?}");
    }
}

/// A registry that holds another algorithm's key under a different `kid`
/// answers exactly like an empty one: no oracle for which keys exist.
#[tokio::test]
async fn an_unknown_and_a_revoked_key_are_indistinguishable() {
    let registry = Arc::new(RegistryVerifierResolver::new());
    registry
        .register(common::p256().verify_key())
        .expect("register");
    let unknown = open(&registry, CoseAlg::Ed25519, CTIS[0]).await;

    registry
        .register(common::ed25519().verify_key())
        .expect("register");
    registry.revoke(&common::ed25519().verify_key().kid());
    let revoked = open(&registry, CoseAlg::Ed25519, CTIS[1]).await;

    assert!(is_coarse_401(&unknown) && is_coarse_401(&revoked));
    assert_eq!(
        common::render(&unknown.unwrap_err()),
        common::render(&revoked.unwrap_err())
    );
}
