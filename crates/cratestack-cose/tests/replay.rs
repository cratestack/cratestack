//! `nonce`-mode freshness and replay (ADR 0006 §5), backend failures
//! (§10), and `kid` collisions (§3).

mod common;

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use common::backends::{FailingNonceStore, FailingResolver, FixedResolver, RecordingNonceStore};
use common::{CTI_16, IAT, rest_request};
use cratestack_core::{CratestackError, InMemoryNonceStore};
use cratestack_cose::{CoseAlg, CoseEnvelope, CoseMode, Ed25519Signer, UNAUTHENTICATED};

fn is_coarse_401<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

#[tokio::test]
async fn the_same_request_twice_is_a_replay() {
    for alg in CoseAlg::ALL {
        let now = common::now();
        let server = common::server(alg, now);
        let sealed = common::sealed_request_at(alg, &rest_request(), now).await;
        server
            .open_request(sealed.clone(), &rest_request())
            .await
            .expect("first delivery");
        let second = server.open_request(sealed, &rest_request()).await;
        assert!(is_coarse_401(&second), "{alg:?}: {second:?}");
    }
}

#[tokio::test]
async fn a_new_cti_is_not_a_replay() {
    let now = common::now();
    let server = common::server(CoseAlg::Ed25519, now);
    for cti in [CTI_16, "00112233445566778899aabbccddeeff"] {
        let sealed = common::client(CoseAlg::Ed25519, now, cti)
            .seal_request(&common::fixture::payment_bytes(), &rest_request())
            .await
            .expect("seal");
        server
            .open_request(sealed, &rest_request())
            .await
            .expect("distinct cti");
    }
}

fn recording_server(now: u64) -> (CoseEnvelope, Arc<RecordingNonceStore>) {
    let store = Arc::new(RecordingNonceStore::default());
    let server = common::server_with(CoseAlg::Ed25519, now, common::resolver(), store.clone());
    (server, store)
}

/// A forged message must not burn the legitimate sender's `cti`.
#[tokio::test]
async fn the_nonce_is_recorded_only_after_verification() {
    let now = common::now();
    let (server, store) = recording_server(now);
    let sealed = common::sealed_request_at(CoseAlg::Ed25519, &rest_request(), now).await;
    let mut forged = sealed.to_vec();
    let last = forged.len() - 1;
    forged[last] ^= 0x01; // same kid and cti, broken signature
    let result = server
        .open_request(Bytes::from(forged), &rest_request())
        .await;
    assert!(is_coarse_401(&result));
    assert_eq!(
        store.calls(),
        0,
        "a failed verification touched the nonce store"
    );

    server
        .open_request(sealed.clone(), &rest_request())
        .await
        .expect("the real message still opens");
    assert_eq!(store.calls(), 1);
    assert!(
        is_coarse_401(&server.open_request(sealed, &rest_request()).await),
        "then it is a replay"
    );
    let (key, expires_at) = store.keys.lock().expect("lock")[0].clone();
    let kid = common::hex(&common::ed25519().verify_key().kid());
    assert_eq!(key, format!("cose:{kid}:{CTI_16}"));
    // Kept for twice the skew (replica clock disagreement), plus a second.
    let iat = i64::try_from(now).expect("fits");
    assert_eq!(expires_at.timestamp(), iat + 2 * 300 + 1);
}

#[tokio::test]
async fn a_stale_or_future_iat_rejects_and_records_nothing() {
    let iat = common::now();
    for (now, fresh) in [
        (iat + 300, true),
        (iat + 301, false),
        (iat - 300, true),
        (iat - 301, false),
    ] {
        let (server, store) = recording_server(now);
        let sealed = common::sealed_request_at(CoseAlg::Ed25519, &rest_request(), iat).await;
        let result = server.open_request(sealed, &rest_request()).await;
        if fresh {
            result.unwrap_or_else(|e| panic!("now = {now}, iat = {iat}: {e:?}"));
            assert_eq!(store.calls(), 1);
        } else {
            assert!(is_coarse_401(&result), "now = {now}: {result:?}");
            assert_eq!(store.calls(), 0);
        }
    }
}

#[tokio::test]
async fn the_skew_is_configurable() {
    let server = CoseEnvelope::server(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .skew(Duration::from_secs(10))
    .clock(|| i64::try_from(IAT + 11).expect("fits"))
    .build()
    .expect("server");
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rest_request()).await;
    assert!(is_coarse_401(
        &server.open_request(sealed, &rest_request()).await
    ));
}

#[tokio::test]
async fn a_failing_resolver_is_a_500_not_a_401() {
    let server = common::server_with(
        CoseAlg::Ed25519,
        IAT,
        Arc::new(FailingResolver),
        Arc::new(InMemoryNonceStore::new()),
    );
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rest_request()).await;
    match server.open_request(sealed, &rest_request()).await {
        Err(error @ CratestackError::Internal(_)) => {
            assert_eq!(error.public_message(), "internal error");
            assert!(error.detail().expect("detail").contains("key resolver"));
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    // A malformed body never reaches the resolver: it is still a 401.
    let garbage = server
        .open_request(Bytes::from_static(b"\xd2\x84"), &rest_request())
        .await;
    assert!(is_coarse_401(&garbage));
}

#[tokio::test]
async fn a_failing_nonce_store_is_a_500_not_a_401() {
    let server = common::server_with(
        CoseAlg::Ed25519,
        IAT,
        common::resolver(),
        Arc::new(FailingNonceStore),
    );
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rest_request()).await;
    match server.open_request(sealed.clone(), &rest_request()).await {
        Err(error @ CratestackError::Internal(_)) => {
            assert!(error.detail().expect("detail").contains("nonce store"));
        }
        other => panic!("expected Internal, got {other:?}"),
    }
    // A forged message is rejected before the store is consulted.
    let mut forged = sealed.to_vec();
    forged[60] ^= 0x01;
    assert!(is_coarse_401(
        &server
            .open_request(Bytes::from(forged), &rest_request())
            .await
    ));
}

/// The resolver returns two candidates for one `kid` (a prefix
/// collision); the second is the signer. The opener tries both.
#[tokio::test]
async fn kid_collision_tries_every_candidate() {
    let other = Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key();
    let right = common::ed25519().verify_key();
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rest_request()).await;
    let server = |keys| {
        common::server_with(
            CoseAlg::Ed25519,
            IAT,
            Arc::new(FixedResolver(keys)),
            Arc::new(InMemoryNonceStore::new()),
        )
    };

    let opened = server(vec![other.clone(), right.clone()])
        .open_request(sealed.clone(), &rest_request())
        .await
        .expect("the second candidate verifies");
    assert_eq!(opened.key_thumbprint, right.thumbprint());
    assert_ne!(opened.key_thumbprint, other.thumbprint());

    let wrong_only = server(vec![other])
        .open_request(sealed.clone(), &rest_request())
        .await;
    assert!(is_coarse_401(&wrong_only));
    let none = server(Vec::new())
        .open_request(sealed, &rest_request())
        .await;
    assert!(is_coarse_401(&none), "an unknown kid is the same 401");
}
