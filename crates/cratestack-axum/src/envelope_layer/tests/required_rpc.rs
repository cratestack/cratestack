//! `Required` over RPC, the same guarantees as REST (transport parity):
//! the op id is the route, `/rpc/batch` is one unary message bound as
//! `batch` (D11), and refusals use the RPC error vocabulary.

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

#[tokio::test]
async fn a_signed_call_is_bound_to_its_op_id_and_its_response_verifies() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "procedure.notify", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/rpc/procedure.notify", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed());
    assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    assert_eq!(answer.seen("x-seen-content-type"), "application/cbor");
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(payload.as_ref(), PAYLOAD);
}

#[tokio::test]
async fn a_call_sealed_for_one_op_does_not_open_at_another() {
    let hits = Hits::default();
    let sealed = Call::new(Method::POST, "procedure.read", &[])
        .seal(PAYLOAD)
        .await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/rpc/procedure.transfer", sealed),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn unsigned_and_tampered_calls_are_the_unsigned_rpc_401() {
    let hits = Hits::default();
    let router = router(&hits);
    let mut tampered = Call::new(Method::POST, "procedure.notify", &[])
        .seal(PAYLOAD)
        .await
        .to_vec();
    tampered[20] ^= 0x40;
    let unsigned = send(
        &router,
        plain_request(Method::POST, "/rpc/procedure.notify", PAYLOAD),
    )
    .await;
    let bad = send(
        &router,
        cose_request(Method::POST, "/rpc/procedure.notify", tampered.into()),
    )
    .await;
    for answer in [&unsigned, &bad] {
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
        assert_eq!(answer.content_type(), "application/cbor", "D4: unsigned");
        assert_eq!(error_code(&answer.body), "unauthenticated");
    }
    assert_eq!(unsigned.body, bad.body);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn the_batch_route_is_one_unary_message_bound_as_batch() {
    let hits = Hits::default();
    let router = router(&hits);
    let call = Call::new(Method::POST, "batch", &[]);
    // The frames are read once opened (decision B1), so the payload is a
    // real frame array.
    let frames = batch_frames(&["procedure.notify"]);
    let sealed = call.seal(&frames).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/rpc/batch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");

    let as_op = Call::new(Method::POST, "procedure.notify", &[])
        .seal(&frames)
        .await;
    let answer = send(&router, cose_request(Method::POST, "/rpc/batch", as_op)).await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_subscription_cannot_stream_unsigned_under_required() {
    let hits = Hits::default();
    let call = Call::new(Method::GET, "subscribe/model.Widget.subscribe", &[]);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(&hits),
        cose_request(
            Method::GET,
            "/rpc/subscribe/model.Widget.subscribe",
            sealed.clone(),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("a sealed 406, not a plain stream");
    assert_eq!(
        error_code(&payload),
        "invalid_argument",
        "RPC vocabulary for a 406"
    );
}
