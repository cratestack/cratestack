//! Streams cannot be sealed until ADR 0006 P1: under `Required` one is
//! replaced by a sealed `406`, under `Optional` it passes plain.

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(mode: EnvelopeMode) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(mode)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, &Hits::default())
}

#[tokio::test]
async fn under_required_a_stream_becomes_a_sealed_406() {
    let call = Call::new(Method::GET, "/stream", &[]);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(EnvelopeMode::Required),
        cose_request(Method::GET, "/stream", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed");
    assert_eq!(error_code(&payload), "NOT_ACCEPTABLE");
}

#[tokio::test]
async fn under_optional_a_stream_passes_plain() {
    let call = Call::new(Method::GET, "/stream", &[]);
    let answer = send(
        &router(EnvelopeMode::Optional),
        cose_request(Method::GET, "/stream", call.seal(&[]).await),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), "application/cbor-seq");
    assert_eq!(answer.body.as_ref(), b"\x01\x02");
}
