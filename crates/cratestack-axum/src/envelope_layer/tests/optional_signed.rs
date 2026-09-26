//! Decision S3: under `Optional`, a signed request always gets a sealed
//! response, exactly as under `Required`. Its `Accept` is not bound, so the
//! layer forces it to CBOR rather than let an on-path party ask for a
//! representation that cannot be sealed and have the answer go out plain.

use cratestack_core::request_digest;
use http::{HeaderValue, Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn optional(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

#[tokio::test]
async fn a_signed_request_that_asks_for_a_stream_is_still_answered_sealed() {
    let call = Call::new(Method::GET, "/stream", &[]);
    let sealed = call.seal(&[]).await;
    let mut req = cose_request(Method::GET, "/stream", sealed.clone());
    req.headers_mut().insert(
        header::ACCEPT,
        HeaderValue::from_static("application/cbor-seq"),
    );
    let answer = send(&optional(&Hits::default()), req).await;
    assert!(
        answer.is_sealed(),
        "{} {}",
        answer.status,
        answer.content_type()
    );
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn a_non_cbor_success_to_a_signed_request_is_a_sealed_500() {
    let call = Call::new(Method::GET, "/json", &[]);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &optional(&Hits::default()),
        cose_request(Method::GET, "/json", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.is_sealed());
    let body = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(error_code(&body), "INTERNAL_ERROR");
}

/// The unsigned half of `Optional` is unchanged: a non-CBOR success to a
/// plain request without a nonce goes out as it came.
#[tokio::test]
async fn a_plain_request_still_gets_its_plain_answer() {
    let answer = send(
        &optional(&Hits::default()),
        plain_request(Method::GET, "/json", b""),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), "application/json");
}

/// The RPC twin (second-review nit): a signed call under `Optional` is
/// answered sealed whatever it asks for; a non-CBOR success becomes a
/// sealed `500` in the RPC vocabulary, and a stream request gets CBOR.
#[tokio::test]
async fn over_rpc_a_signed_call_under_optional_is_always_answered_sealed() {
    use axum::routing::post;
    let layer = || {
        EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
            .policy(EnvelopeMode::Optional)
            .rpc("")
            .build()
            .expect("layer")
    };
    let json = axum::Router::new()
        .route(
            "/rpc/{op_id}",
            post(|| async {
                let mut response = axum::response::Response::new(axum::body::Body::from("{}"));
                response.headers_mut().insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("application/json"),
                );
                response
            }),
        )
        .layer(layer());
    let call = Call::new(Method::POST, "procedure.json", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &json,
        cose_request(Method::POST, "/rpc/procedure.json", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(answer.is_sealed());
    let body = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(error_code(&body), "internal", "RPC vocabulary");

    let hits = Hits::default();
    let echo = super::fixtures::rpc_router(layer(), &hits);
    let call = Call::new(Method::POST, "procedure.notify", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let mut req = cose_request(Method::POST, "/rpc/procedure.notify", sealed.clone());
    req.headers_mut().insert(
        header::ACCEPT,
        HeaderValue::from_static("application/cbor-seq"),
    );
    let answer = send(&echo, req).await;
    assert_eq!(answer.seen("x-seen-accept"), "application/cbor");
    assert_eq!(answer.seen("x-seen-principal"), CLIENT_PRINCIPAL);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(hits.get(), 1);
}
