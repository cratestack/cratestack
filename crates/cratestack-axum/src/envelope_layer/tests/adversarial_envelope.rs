//! An envelope's errors never reach the wire, a failing signer never
//! produces a plain "success", and the builder refuses a layer that would
//! bind nothing.

use cratestack_core::CratestackError;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use super::toy::{Toy, toy_request};
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

async fn answer_with(toy: Toy, body: &[u8]) -> (Answer, Hits) {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(toy, AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let req = toy_request(Method::POST, "/widgets", "application/cose", body);
    (send(&rest_router(layer, &hits), req).await, hits)
}

#[tokio::test]
async fn a_verification_failure_is_the_coarse_401_whatever_the_envelope_said() {
    let (answer, hits) = answer_with(Toy::default(), b"NOPE").await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    let text = String::from_utf8_lossy(&answer.body);
    assert!(
        !text.contains("byte 0"),
        "the envelope's detail leaked: {text}"
    );
    assert!(text.contains(cratestack_cose::UNAUTHENTICATED));
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn any_other_envelope_error_is_a_500_without_its_detail() {
    fn bad_request() -> CratestackError {
        CratestackError::BadRequest("secret-detail".to_owned())
    }
    fn unavailable() -> CratestackError {
        CratestackError::Unavailable("secret-detail".to_owned())
    }
    for error in [bad_request as fn() -> CratestackError, unavailable] {
        let toy = Toy {
            open_error: Some(error),
            ..Toy::default()
        };
        let (answer, hits) = answer_with(toy, b"TOY:x").await;
        assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(!String::from_utf8_lossy(&answer.body).contains("secret"));
        assert_eq!(hits.get(), 0);
    }
}

#[tokio::test]
async fn a_failing_signer_is_an_unsigned_500_never_a_plain_success() {
    let toy = Toy {
        seal_fails: true,
        ..Toy::default()
    };
    let (answer, hits) = answer_with(toy, b"TOY:x").await;
    assert_eq!(
        hits.get(),
        1,
        "the request ran; only its answer could not be signed"
    );
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(answer.content_type(), "application/cbor");
    assert!(!String::from_utf8_lossy(&answer.body).contains("hsm"));
}

#[tokio::test]
async fn a_body_over_the_layers_cap_is_refused_unsigned() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(Toy::default(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .max_body_bytes(8)
        .build()
        .expect("layer");
    let req = toy_request(Method::POST, "/widgets", "application/cose", &[b'x'; 64]);
    let answer = send(&rest_router(layer, &hits), req).await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(hits.get(), 0);
}

#[test]
fn the_builder_refuses_a_layer_that_would_bind_nothing() {
    let base = || {
        EnvelopeLayer::builder(Toy::default(), AUDIENCE, SCHEMA)
            .policy(EnvelopeMode::Required)
            .rest("", &REST_ROUTES)
    };
    assert!(base().build().is_ok());
    let empty_audience = EnvelopeLayer::builder(Toy::default(), "", SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES);
    let no_policy = EnvelopeLayer::builder(Toy::default(), AUDIENCE, SCHEMA).rest("", &REST_ROUTES);
    let no_transport =
        EnvelopeLayer::builder(Toy::default(), AUDIENCE, SCHEMA).policy(EnvelopeMode::Required);
    for builder in [
        empty_audience,
        no_policy,
        no_transport,
        base().max_body_bytes(0),
    ] {
        let error = builder.build().expect_err("refused");
        assert_eq!(error.status_code(), StatusCode::UNPROCESSABLE_ENTITY);
    }
}
