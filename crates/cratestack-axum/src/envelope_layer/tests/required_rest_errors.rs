//! `Required` over REST: every error of a generated op is signed with its
//! status, and nothing leaves plain.

use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    rest_router(layer, hits)
}

async fn signed_get(
    path: &'static str,
    route: &'static str,
    params: &[&'static str],
) -> (Call, bytes::Bytes, Answer) {
    let call = Call::new(Method::GET, route, params);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &router(&Hits::default()),
        cose_request(Method::GET, path, sealed.clone()),
    )
    .await;
    (call, sealed, answer)
}

#[tokio::test]
async fn a_handler_error_is_signed_with_its_status() {
    let (call, sealed, answer) = signed_get("/widgets/404", "/widgets/{id}", &["404"]).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
    assert!(answer.is_sealed());
    let digest = request_digest(&sealed);
    call.open(digest, StatusCode::OK, answer.body.clone())
        .await
        .expect_err("the status is bound: a 404 is not a 200");
    let payload = call
        .open(digest, answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(error_code(&payload), "NOT_FOUND");
}

#[tokio::test]
async fn a_text_plain_error_is_re_encoded_as_cbor_and_signed() {
    let (call, sealed, answer) = signed_get("/text-error", "/text-error", &[]).await;
    assert_eq!(
        answer.status,
        StatusCode::PAYLOAD_TOO_LARGE,
        "the status is kept"
    );
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(error_code(&payload), "INTERNAL_ERROR");
}

#[tokio::test]
async fn a_non_cbor_success_is_replaced_by_a_signed_500_never_sent_plain() {
    let (call, sealed, answer) = signed_get("/json", "/json", &[]).await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(error_code(&payload), "INTERNAL_ERROR");
}

/// axum serves `HEAD` from the `GET` route; it is still a generated op, so
/// under `Required` it must be signed too (D3), not pass as "unresolved".
#[tokio::test]
async fn head_is_signed_like_get() {
    let hits = Hits::default();
    let router = router(&hits);
    let unsigned = send(&router, plain_request(Method::HEAD, "/widgets/1", b"")).await;
    assert_eq!(unsigned.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);

    let call = Call::new(Method::HEAD, "/widgets/{id}", &["1"]);
    let signed = send(
        &router,
        cose_request(Method::HEAD, "/widgets/1", call.seal(&[]).await),
    )
    .await;
    assert_eq!(signed.status, StatusCode::OK);
    assert!(
        signed.is_sealed(),
        "sealed, although axum strips a HEAD body"
    );
    assert_eq!(hits.get(), 1);
}
