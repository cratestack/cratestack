//! `nonce`-mode freshness and replay (ADR 0006 §5), backend failures
//! (§10), and `kid` collisions (§3).

mod common;

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use common::backends::{FailingNonceStore, FailingResolver, RecordingNonceStore};
use common::{CTI_16, IAT, rest_request, unhex};
use cratestack_core::{CratestackContext, CratestackEnvelope, CratestackError, InMemoryNonceStore};
use cratestack_cose::{
    CoseAlg, CoseEnvelope, CoseMode, CoseSigner, CoseVerifyKey, HmacSigner, StaticVerifierResolver,
    UNAUTHENTICATED,
};

fn is_coarse_401<T: std::fmt::Debug>(result: &Result<T, CratestackError>) -> bool {
    matches!(result, Err(CratestackError::Unauthorized(message)) if message == UNAUTHENTICATED)
}

#[tokio::test]
async fn the_same_request_twice_is_a_replay() {
    for &alg in CoseAlg::ALL {
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

/// A skew whose nonce expiry (`iat + 2·skew + 1`) would leave the time
/// range is refused when the envelope is built (security review of
/// cratestack#1005, fix 5). Before, it built, and then every valid request
/// was a `401`, because the expiry could not be computed.
#[test]
fn a_skew_that_overflows_the_nonce_expiry_is_refused_at_build() {
    for skew in [
        u64::MAX,
        u64::try_from(i64::MAX).expect("fits"),
        10_000_000_000_000,
    ] {
        let built = CoseEnvelope::server(
            CoseMode::Sign1,
            common::signer(CoseAlg::Ed25519),
            common::resolver(),
            Arc::new(InMemoryNonceStore::new()),
        )
        .skew(Duration::from_secs(skew))
        .build();
        match built {
            Err(CratestackError::Validation(message)) => assert!(message.contains("skew")),
            other => panic!("skew {skew} built: {other:?}"),
        }
    }
}

/// A large but representable skew still builds, and a valid request opens
/// under it.
#[tokio::test]
async fn a_large_representable_skew_builds_and_works() {
    let now = common::now();
    let ten_years = 10 * 366 * 24 * 60 * 60;
    let server = CoseEnvelope::server(
        CoseMode::Sign1,
        common::signer(CoseAlg::Ed25519),
        common::resolver(),
        Arc::new(InMemoryNonceStore::new()),
    )
    .skew(Duration::from_secs(ten_years))
    .clock(move || i64::try_from(now).expect("fits"))
    .build()
    .expect("ten years of skew is representable");
    let sealed = common::sealed_request_at(CoseAlg::Ed25519, &rest_request(), now).await;
    server
        .open_request(sealed, &rest_request())
        .await
        .expect("a valid request opens");
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

/// Two 32-byte HMAC secrets whose RFC 9679 thumbprints share their first
/// 8 bytes, so both keys have the `kid` `aee74241c1a4ec95`: a real 64-bit
/// prefix collision, not a resolver pretending one exists. Found with a
/// parallel Pollard-rho search (distinguished points) over
/// `x ↦ kid(secret(x))`, where `secret(x)` is the 8 big-endian bytes of `x`
/// repeated four times: about 6.4·10⁹ SHA-256 evaluations, 20 s on 16
/// threads. The test recomputes both thumbprints, so it depends on nothing
/// but these bytes.
const COLLIDING_A: &str = "995acdc2fbb6599b995acdc2fbb6599b995acdc2fbb6599b995acdc2fbb6599b";
const COLLIDING_B: &str = "e9937b54005bf2c1e9937b54005bf2c1e9937b54005bf2c1e9937b54005bf2c1";

/// Two keys share a `kid`; the resolver files each under its own computed
/// `kid` and so returns both. The message is B's, B is the second
/// candidate: the opener must try past the first, and the principal must
/// be B's thumbprint, not A's.
#[tokio::test]
async fn kid_collision_tries_every_candidate() {
    let a = HmacSigner::new(CoseAlg::Hmac256_256, unhex(COLLIDING_A)).expect("key a");
    let b = HmacSigner::new(CoseAlg::Hmac256_256, unhex(COLLIDING_B)).expect("key b");
    let (a_key, b_key) = (a.verify_key(), b.verify_key());
    assert_eq!(a.kid(), b.kid(), "a real kid collision");
    assert_eq!(a_key.kid(), common::unhex("aee74241c1a4ec95").as_slice());
    assert_ne!(a_key.thumbprint(), b_key.thumbprint(), "two different keys");

    let now = common::now();
    let server = |keys: Vec<CoseVerifyKey>| {
        let resolver = keys.into_iter().fold(
            StaticVerifierResolver::new(),
            StaticVerifierResolver::with_key,
        );
        common::server_with(
            CoseAlg::Hmac256_256,
            now,
            Arc::new(resolver),
            Arc::new(InMemoryNonceStore::new()),
        )
    };
    let seal_by = |signer: HmacSigner, cti: &'static str| async move {
        CoseEnvelope::client(CoseMode::Mac0, Arc::new(signer), common::resolver())
            .clock(move || i64::try_from(now).expect("fits"))
            .cti_source(move || Ok(common::unhex(cti)))
            .build()
            .expect("client")
            .seal_request(&common::fixture::payment_bytes(), &rest_request())
            .await
            .expect("seal")
    };
    let by_b = seal_by(b.clone(), CTI_16).await;

    let mut ctx = CratestackContext::anonymous();
    CratestackEnvelope::open(
        &server(vec![a_key.clone(), b_key.clone()]),
        by_b.clone(),
        &rest_request(),
        &mut ctx,
    )
    .await
    .expect("the second candidate verifies");
    let signer = ctx.verified_signer().expect("recorded");
    assert_eq!(
        signer.thumbprint(),
        &b_key.thumbprint(),
        "the principal is B"
    );
    assert_eq!(signer.kid(), a.kid(), "under the shared kid");

    // A's own message, same resolver: the first candidate, A.
    let by_a = seal_by(a, "00112233445566778899aabbccddeeff").await;
    let opened = server(vec![a_key.clone(), b_key.clone()])
        .open_request(by_a, &rest_request())
        .await
        .expect("A verifies");
    assert_eq!(opened.thumbprint, a_key.thumbprint());

    // B's message where only A is known: the shared kid is not enough.
    let wrong_only = server(vec![a_key])
        .open_request(by_b.clone(), &rest_request())
        .await;
    assert!(is_coarse_401(&wrong_only));
    let none = server(Vec::new()).open_request(by_b, &rest_request()).await;
    assert!(is_coarse_401(&none), "an unknown kid is the same 401");
}
