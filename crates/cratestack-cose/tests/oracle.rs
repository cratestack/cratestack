//! No oracle (ADR 0006 §10): every failed check produces the same error,
//! byte for byte, whatever it was that failed.

mod common;

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use common::backends::FixedResolver;
use common::forge::{self, with_unprotected};
use common::{CTI_16, IAT, rest_request, unhex};
use cratestack_core::{Binding, CratestackError, InMemoryNonceStore};
use cratestack_cose::{CoseAlg, Ed25519Signer, UNAUTHENTICATED, external_aad};

#[tokio::test]
async fn every_verification_failure_is_the_same_error() {
    let payload = common::fixture::payment_bytes();
    let sealed = common::sealed_request(CoseAlg::Ed25519, &rest_request())
        .await
        .to_vec();
    let aad = external_aad(&rest_request()).expect("aad");
    let mut failures: Vec<(&str, CratestackError)> = Vec::new();
    let server = common::server(CoseAlg::Ed25519, IAT);
    let open = |body: Vec<u8>, bind: Binding<'static>| {
        let server = server.clone();
        async move {
            server
                .open_request(Bytes::from(body), &bind)
                .await
                .expect_err("must fail")
        }
    };

    failures.push(("empty", open(Vec::new(), rest_request()).await));
    failures.push((
        "garbage",
        open(b"not cbor at all".to_vec(), rest_request()).await,
    ));
    let mut trailing = sealed.clone();
    trailing.push(0);
    failures.push(("trailing", open(trailing, rest_request()).await));
    failures.push((
        "unprotected",
        open(
            with_unprotected(&sealed, &unhex("a1 03 00")),
            rest_request(),
        )
        .await,
    ));
    let mut bad_sig = sealed.clone();
    let last = bad_sig.len() - 1;
    bad_sig[last] ^= 1;
    failures.push(("signature", open(bad_sig, rest_request()).await));
    let mut bad_payload = sealed.clone();
    bad_payload[100] ^= 1;
    failures.push(("payload", open(bad_payload, rest_request()).await));
    let other_route = Binding {
        route: Cow::Borrowed("model.Payment.refund"),
        ..rest_request()
    };
    failures.push(("aad", open(sealed.clone(), other_route).await));
    let mut retagged = sealed.clone();
    retagged[0] = forge::TAG_MAC0;
    failures.push(("tag", open(retagged, rest_request()).await));
    let deprecated = forge::request_protected(
        -8,
        common::ed25519().verify_key().kid().as_slice(),
        u32::try_from(IAT).expect("u32"),
        &unhex(CTI_16),
    );
    failures.push((
        "alg -8",
        open(
            forge::ed25519_request(&deprecated, &aad, &payload),
            rest_request(),
        )
        .await,
    ));

    // Unknown kid: a validly signed message from a key the resolver lacks.
    let stranger = cratestack_cose::CoseEnvelope::client(
        cratestack_cose::CoseMode::Sign1,
        Arc::new(Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED)),
        common::resolver(),
    )
    .clock(|| i64::try_from(IAT).expect("fits"))
    .build()
    .expect("client");
    let unknown = stranger
        .seal_request(&payload, &rest_request())
        .await
        .expect("seal");
    failures.push(("unknown kid", open(unknown.to_vec(), rest_request()).await));

    // Wrong key for a known kid (a collision candidate that fails).
    let wrong = common::server_with(
        CoseAlg::Ed25519,
        IAT,
        Arc::new(FixedResolver(vec![
            Ed25519Signer::from_seed(&common::OTHER_ED25519_SEED).verify_key(),
        ])),
        Arc::new(InMemoryNonceStore::new()),
    );
    failures.push((
        "wrong key",
        wrong
            .open_request(Bytes::from(sealed.clone()), &rest_request())
            .await
            .expect_err("fails"),
    ));

    // Stale.
    let late = common::server(CoseAlg::Ed25519, IAT + 10_000);
    failures.push((
        "stale",
        late.open_request(Bytes::from(sealed.clone()), &rest_request())
            .await
            .expect_err("fails"),
    ));

    // Replay, on real time (see `common::now`).
    let now = common::now();
    let live = common::server(CoseAlg::Ed25519, now);
    let fresh = common::sealed_request_at(CoseAlg::Ed25519, &rest_request(), now).await;
    live.open_request(fresh.clone(), &rest_request())
        .await
        .expect("first delivery");
    failures.push((
        "replay",
        live.open_request(fresh, &rest_request())
            .await
            .expect_err("replay"),
    ));

    let expected = common::render(&CratestackError::Unauthorized(UNAUTHENTICATED.to_owned()));
    for (what, error) in &failures {
        assert_eq!(common::render(error), expected, "{what} differs");
    }
    assert!(failures.len() >= 13);
}
