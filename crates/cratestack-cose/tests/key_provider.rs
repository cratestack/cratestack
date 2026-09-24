//! `KeyProviderMacKeys`: Mac0 keys from core's `KeyProvider`, by string
//! key id, resolved and thumbprinted once (ADR 0006 §1 adapters; P0
//! scoping decision "deployers list their key ids").

mod common;

use std::sync::Arc;

use common::{rest_request, unhex};
use cratestack_core::{CratestackError, InMemoryNonceStore, KeyProvider, StaticKeyProvider};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, CoseVerifierResolver, KeyProviderMacKeys,
    UNAUTHENTICATED,
};

fn provider() -> StaticKeyProvider {
    StaticKeyProvider::new()
        .with_key("svc-2026-09", common::HMAC_SECRET.to_vec())
        .with_key("svc-2026-10", vec![0x77; 48])
        .with_key("short", vec![0x11; 31])
        .with_key("alias", common::HMAC_SECRET.to_vec())
}

async fn load(ids: &[&str]) -> Result<KeyProviderMacKeys, CratestackError> {
    KeyProviderMacKeys::load(&provider(), CoseAlg::Hmac256_256, ids.iter().copied()).await
}

#[tokio::test]
async fn keys_are_resolved_and_filed_under_their_thumbprint_kid() {
    let keys = load(&["svc-2026-09", "svc-2026-10"]).await.expect("load");
    assert_eq!(keys.alg(), CoseAlg::Hmac256_256);
    let listed: Vec<_> = keys.kids().collect();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].0, "svc-2026-09");
    assert_eq!(
        listed[0].1,
        common::hmac(CoseAlg::Hmac256_256).verify_key().kid()
    );

    let kid = listed[0].1;
    let found = keys
        .resolve(&kid, CoseAlg::Hmac256_256)
        .await
        .expect("resolve");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0], common::hmac(CoseAlg::Hmac256_256).verify_key());
    // The set's keys carry its algorithm and no other.
    assert!(
        keys.resolve(&kid, CoseAlg::Hmac256_64)
            .await
            .expect("resolve")
            .is_empty()
    );
}

#[tokio::test]
async fn an_unknown_kid_is_an_empty_answer_not_an_error() {
    let keys = load(&["svc-2026-09"]).await.expect("load");
    let found = keys
        .resolve(&unhex("0001020304050607"), CoseAlg::Hmac256_256)
        .await
        .expect("never Err");
    assert!(found.is_empty());
}

#[tokio::test]
async fn the_signer_and_the_resolver_work_end_to_end() {
    let keys = Arc::new(load(&["svc-2026-09", "svc-2026-10"]).await.expect("load"));
    let now = common::now();
    let clock = move || i64::try_from(now).expect("fits");
    let signer = keys.signer("svc-2026-10").expect("configured");
    assert_eq!(signer.alg(), CoseAlg::Hmac256_256);
    assert!(keys.signer("svc-2099-01").is_none());
    let client = CoseEnvelope::client(CoseMode::Mac0, Arc::new(signer.clone()), keys.clone())
        .clock(clock)
        .build()
        .expect("client");
    let server = CoseEnvelope::server(
        CoseMode::Mac0,
        Arc::new(signer),
        keys,
        Arc::new(InMemoryNonceStore::new()),
    )
    .clock(clock)
    .build()
    .expect("server");
    let sealed = client
        .seal_request(&common::fixture::payment_bytes(), &rest_request())
        .await
        .expect("seal");
    server
        .open_request(sealed, &rest_request())
        .await
        .expect("opens");

    // A message under a secret the set does not hold is the coarse 401.
    let stranger = common::sealed_request_at(CoseAlg::Hmac256_256, &rest_request(), now).await;
    let only_october = Arc::new(load(&["svc-2026-10"]).await.expect("load"));
    let result = common::server_with(
        CoseAlg::Hmac256_256,
        now,
        only_october,
        Arc::new(InMemoryNonceStore::new()),
    )
    .open_request(stranger, &rest_request())
    .await;
    assert!(matches!(result, Err(CratestackError::Unauthorized(m)) if m == UNAUTHENTICATED));
}

fn validation(result: Result<KeyProviderMacKeys, CratestackError>) -> String {
    match result {
        Err(CratestackError::Validation(message)) => message,
        other => panic!("expected a validation error, got {other:?}"),
    }
}

#[tokio::test]
async fn misconfiguration_fails_loudly_at_load() {
    assert!(validation(load(&["short"]).await).contains("under 32 bytes"));
    assert!(validation(load(&[]).await).contains("at least one"));
    assert!(validation(load(&["svc-2026-09", "svc-2026-09"]).await).contains("twice"));
    assert!(validation(load(&["svc-2026-09", "alias"]).await).contains("same secret"));
    let wrong_alg = KeyProviderMacKeys::load(&provider(), CoseAlg::Ed25519, ["svc-2026-09"]).await;
    assert!(validation(wrong_alg).contains("HMAC algorithm"));
    // A secret's bytes never appear in an error.
    let message = validation(load(&["short"]).await);
    assert!(
        !message.contains("11, 11") && !message.contains("1111"),
        "{message}"
    );
}

#[tokio::test]
async fn a_provider_error_is_returned_as_is() {
    let missing = load(&["svc-2099-01"]).await;
    let direct = provider().resolve_signing_key("svc-2099-01").await;
    assert!(missing.is_err());
    assert_eq!(
        format!("{:?}", missing.expect_err("missing")),
        format!("{:?}", direct.expect_err("missing"))
    );
}
