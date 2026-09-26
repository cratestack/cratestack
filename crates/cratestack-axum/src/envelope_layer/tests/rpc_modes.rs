//! `Optional` and `Off` over RPC, the twins of `optional.rs` and
//! `off_mode.rs` (transport parity): the modes behave the same whichever
//! binding carries the call, and refusals speak the RPC vocabulary.

use axum::body::Body;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest, request_digest_unsigned};
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(mode: EnvelopeMode, hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(mode)
        .rpc("")
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

#[tokio::test]
async fn optional_opens_a_signed_call_and_lets_a_plain_one_run() {
    let hits = Hits::default();
    let router = router(EnvelopeMode::Optional, &hits);
    let call = Call::new(Method::POST, "procedure.notify", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let signed = send(
        &router,
        cose_request(Method::POST, "/rpc/procedure.notify", sealed.clone()),
    )
    .await;
    assert_eq!(signed.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    call.open(request_digest(&sealed), signed.status, signed.body)
        .await
        .expect("verifies");

    let plain = send(
        &router,
        plain_request(Method::POST, "/rpc/procedure.notify", PAYLOAD),
    )
    .await;
    assert_eq!(plain.status, StatusCode::OK);
    assert!(!plain.is_sealed());
    assert_eq!(plain.seen("x-seen-principal"), "<absent>");
    assert_eq!(hits.get(), 2);
}

#[tokio::test]
async fn optional_seals_a_nonce_bound_plain_call_bound_to_its_nonce_and_body() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([5; 16]);
    let req = http::Request::builder()
        .method(Method::POST)
        .uri("/rpc/procedure.notify")
        .header(header::CONTENT_TYPE, "application/cbor")
        .header(header::ACCEPT, SIGN1)
        .header(NONCE_HEADER, nonce.to_header_value())
        .body(Body::from(PAYLOAD))
        .expect("request");
    let answer = send(&router(EnvelopeMode::Optional, &hits), req).await;
    assert!(answer.is_sealed());
    let call = Call::new(Method::POST, "procedure.notify", &[]);
    call.open(
        request_digest_unsigned(&nonce, b"other body"),
        answer.status,
        answer.body.clone(),
    )
    .await
    .expect_err("bound to the body that was sent");
    call.open(
        request_digest_unsigned(&nonce, PAYLOAD),
        answer.status,
        answer.body,
    )
    .await
    .expect("verifies");
}

#[tokio::test]
async fn optional_refuses_a_bad_signature_with_the_rpc_401() {
    let hits = Hits::default();
    let answer = send(
        &router(EnvelopeMode::Optional, &hits),
        cose_request(
            Method::POST,
            "/rpc/procedure.notify",
            bytes::Bytes::from_static(PAYLOAD),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(error_code(&answer.body), "unauthenticated");
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn off_passes_plain_calls_and_refuses_cose_bodies() {
    let hits = Hits::default();
    let router = router(EnvelopeMode::Off, &hits);
    let plain = send(
        &router,
        plain_request(Method::POST, "/rpc/procedure.notify", PAYLOAD),
    )
    .await;
    assert_eq!(plain.status, StatusCode::OK);
    assert!(!plain.is_sealed());

    let sealed = Call::new(Method::POST, "procedure.notify", &[])
        .seal(PAYLOAD)
        .await;
    let cose = send(
        &router,
        cose_request(Method::POST, "/rpc/procedure.notify", sealed),
    )
    .await;
    assert_eq!(cose.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_code(&cose.body), "invalid_argument");
    assert_eq!(hits.get(), 1);
}
