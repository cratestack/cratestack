//! `Optional`: an unsigned, nonce-bound request negotiates its response type
//! the same way (cratestack#1168).

use axum::body::Body;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest_unsigned};
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::payload_types_support::*;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .payload_media_types([CBOR], [CBOR, JSON])
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

fn unsigned_post(nonce: &RequestNonce, accept: Option<&str>) -> axum::extract::Request {
    let mut builder = http::Request::builder()
        .method(Method::POST)
        .uri("/either")
        .header(header::ACCEPT, SIGN1)
        .header(header::CONTENT_TYPE, CBOR)
        .header(NONCE_HEADER, nonce.to_header_value());
    if let Some(accept) = accept {
        builder = builder.header(cratestack_core::PAYLOAD_ACCEPT_HEADER, accept);
    }
    builder.body(Body::from(PAYLOAD)).expect("request")
}

#[tokio::test]
async fn an_unsigned_nonce_bound_request_asking_for_json_gets_a_sealed_json_answer() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([9; 16]);
    let answer = send(&router(&hits), unsigned_post(&nonce, Some(JSON))).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed());
    assert_eq!(answer.seen("x-seen-accept"), JSON);
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
    let call = Call::new(Method::POST, "/either", &[]).types(CBOR, JSON);
    call.open(
        request_digest_unsigned(&nonce, PAYLOAD),
        answer.status,
        answer.body,
    )
    .await
    .expect("verifies under JSON, bound to its nonce");
}

#[tokio::test]
async fn without_the_header_an_unsigned_request_is_answered_in_cbor_as_before() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([9; 16]);
    let answer = send(&router(&hits), unsigned_post(&nonce, None)).await;
    assert!(answer.is_sealed());
    assert_eq!(answer.seen("x-seen-accept"), CBOR);
    assert_eq!(answer.seen("cratestack-payload-type"), CBOR);
    Call::new(Method::POST, "/either", &[])
        .open(
            request_digest_unsigned(&nonce, PAYLOAD),
            answer.status,
            answer.body,
        )
        .await
        .expect("verifies under CBOR");
}

#[tokio::test]
async fn an_unsigned_request_naming_nothing_acceptable_is_the_unsigned_406() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([9; 16]);
    let answer = send(
        &router(&hits),
        unsigned_post(&nonce, Some("application/x-www-form-urlencoded")),
    )
    .await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
}
