//! `Optional` (decision D10): signed requests are opened and answered
//! sealed; unsigned ones run, and are answered sealed only when they carry
//! a valid `Cratestack-Nonce` and ask for `application/cose`.

use axum::body::Body;
use cratestack_cose::{NONCE_HEADER, RequestNonce, request_digest, request_digest_unsigned};
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

fn unsigned_get(nonce: Option<&str>, accept: &str) -> axum::extract::Request {
    let mut builder = http::Request::builder()
        .method(Method::GET)
        .uri("/widgets/1")
        .header(header::ACCEPT, accept);
    if let Some(nonce) = nonce {
        builder = builder.header(NONCE_HEADER, nonce);
    }
    builder.body(Body::empty()).expect("request")
}

#[tokio::test]
async fn a_signed_request_is_opened_and_answered_sealed() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/widgets", sealed.clone()),
    )
    .await;
    assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn a_signed_request_that_fails_is_401_never_downgraded_to_unsigned() {
    let hits = Hits::default();
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/widgets", bytes::Bytes::from_static(PAYLOAD)),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn an_unsigned_request_runs_plain_and_untouched() {
    let hits = Hits::default();
    let answer = send(
        &router(&hits),
        plain_request(Method::POST, "/widgets", PAYLOAD),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), "application/cbor");
    assert_eq!(answer.seen("x-seen-principal"), "<absent>");
    assert_eq!(answer.body.as_ref(), PAYLOAD);
}

#[tokio::test]
async fn a_nonce_bound_get_is_answered_sealed_and_bound_to_its_nonce() {
    let hits = Hits::default();
    let nonce = RequestNonce::from_bytes([9; 16]);
    let answer = send(
        &router(&hits),
        unsigned_get(Some(&nonce.to_header_value()), SIGN1),
    )
    .await;
    assert!(answer.is_sealed());
    assert_eq!(answer.seen("x-seen-accept"), "application/cbor");
    let call = Call::new(Method::GET, "/widgets/{id}", &["1"]);
    let other = RequestNonce::from_bytes([8; 16]);
    call.open(
        request_digest_unsigned(&other, &[]),
        answer.status,
        answer.body.clone(),
    )
    .await
    .expect_err("bound to the nonce the client sent, not another");
    call.open(
        request_digest_unsigned(&nonce, &[]),
        answer.status,
        answer.body,
    )
    .await
    .expect("verifies with its own nonce");
}

#[tokio::test]
async fn without_a_valid_nonce_or_a_cose_accept_the_answer_is_plain() {
    let nonce = RequestNonce::from_bytes([9; 16]).to_header_value();
    for (nonce, accept) in [
        (None, SIGN1),
        (Some("not-a-nonce"), SIGN1),
        (Some(nonce.as_str()), "application/cbor"),
        (
            Some(nonce.as_str()),
            "application/cose;q=0, application/cbor",
        ),
    ] {
        let hits = Hits::default();
        let answer = send(&router(&hits), unsigned_get(nonce, accept)).await;
        assert!(!answer.is_sealed(), "{nonce:?} {accept}");
        assert_eq!(hits.get(), 1);
    }
}

#[tokio::test]
async fn a_signed_request_asking_for_a_stream_keeps_its_accept() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "/widgets", &[]);
    let mut req = cose_request(Method::POST, "/widgets", call.seal(PAYLOAD).await);
    req.headers_mut().insert(
        header::ACCEPT,
        http::HeaderValue::from_static("application/cbor-seq"),
    );
    let answer = send(&router(&hits), req).await;
    assert_eq!(answer.seen("x-seen-accept"), "application/cbor-seq");
}
