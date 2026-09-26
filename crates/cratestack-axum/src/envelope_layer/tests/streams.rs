//! Streams cannot be sealed until ADR 0006 P1: a stream answering a signed
//! request, or any request under `Required`, is replaced by a sealed `406`;
//! only an unsigned request under `Optional` streams (plain).

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

/// Decision S3: a signed request is answered sealed under `Optional` too,
/// so a stream it reaches is the sealed `406`, never a plain stream. Only
/// unsigned traffic streams under `Optional`.
#[tokio::test]
async fn under_optional_a_signed_request_never_streams_plain() {
    let call = Call::new(Method::GET, "/stream", &[]);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(EnvelopeMode::Optional),
        cose_request(Method::GET, "/stream", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(answer.is_sealed());
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("a sealed 406");
}

#[tokio::test]
async fn under_optional_an_unsigned_stream_passes_plain() {
    let answer = send(
        &router(EnvelopeMode::Optional),
        plain_request(Method::GET, "/stream", b""),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), "application/cbor-seq");
    assert_eq!(answer.body.as_ref(), b"\x01\x02");
}
