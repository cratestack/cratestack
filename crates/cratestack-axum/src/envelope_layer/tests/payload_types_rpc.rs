//! The same negotiation over `transport rpc` (transport parity,
//! cratestack#1168): a JSON op, the refusals in the RPC vocabulary, and
//! `/rpc/batch`, which stays CBOR.

use cratestack_core::{PAYLOAD_TYPE_NOT_ACCEPTABLE_CODE, PAYLOAD_TYPE_UNSUPPORTED_CODE};
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, JSON_ANSWER, rpc_router};
use super::payload_types_support::*;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn router(hits: &Hits) -> axum::Router {
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .payload_media_types([CBOR, JSON], [CBOR, JSON])
        .build()
        .expect("layer");
    rpc_router(layer, hits)
}

#[tokio::test]
async fn a_json_op_is_sealed_both_ways() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "procedure.json", &[]).types(JSON, JSON);
    let sealed = call.seal(b"{\"a\":1}").await;
    let answer = send(
        &router(&hits),
        typed_request(
            Method::POST,
            "/rpc/procedure.json",
            sealed.clone(),
            Some(JSON),
            Some(JSON),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert!(answer.is_sealed());
    assert_eq!(answer.seen("x-seen-content-type"), JSON);
    assert_eq!(answer.seen("x-seen-accept"), JSON);
    assert_eq!(answer.seen("cratestack-payload-type"), JSON);
    let payload = call
        .open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(payload.as_ref(), JSON_ANSWER.as_bytes());
}

#[tokio::test]
async fn a_cbor_client_is_unchanged_against_the_same_layer() {
    let hits = Hits::default();
    let call = Call::new(Method::POST, "procedure.notify", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router(&hits),
        cose_request(Method::POST, "/rpc/procedure.notify", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-content-type"), CBOR);
    assert_eq!(answer.seen("x-seen-accept"), CBOR);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn refusals_speak_the_rpc_vocabulary() {
    let hits = Hits::default();
    let sealed = Call::new(Method::POST, "procedure.notify", &[])
        .seal(PAYLOAD)
        .await;
    let unsupported = send(
        &router(&hits),
        typed_request(
            Method::POST,
            "/rpc/procedure.notify",
            sealed.clone(),
            Some(FORM),
            None,
        ),
    )
    .await;
    assert_eq!(unsupported.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_code(&unsupported.body), PAYLOAD_TYPE_UNSUPPORTED_CODE);
    let not_acceptable = send(
        &router(&hits),
        typed_request(
            Method::POST,
            "/rpc/procedure.notify",
            sealed.clone(),
            None,
            Some(FORM),
        ),
    )
    .await;
    assert_eq!(not_acceptable.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(
        error_code(&not_acceptable.body),
        PAYLOAD_TYPE_NOT_ACCEPTABLE_CODE
    );
    assert!(!unsupported.is_sealed() && !not_acceptable.is_sealed());
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_lying_type_header_is_the_coarse_401() {
    let hits = Hits::default();
    let sealed = Call::new(Method::POST, "procedure.json", &[])
        .seal(PAYLOAD)
        .await;
    let answer = send(
        &router(&hits),
        typed_request(
            Method::POST,
            "/rpc/procedure.json",
            sealed,
            Some(JSON),
            None,
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 0);
}

fn batch_call() -> Call {
    Call::new(Method::POST, "batch", &[])
}

#[tokio::test]
async fn batch_stays_cbor_only_both_ways() {
    let hits = Hits::default();
    let router = router(&hits);
    let sealed = batch_call().seal(&batch_frames(&["procedure.read"])).await;

    // A non-CBOR batch is refused, even where the layer allows JSON.
    let json = send(
        &router,
        typed_request(Method::POST, "/rpc/batch", sealed.clone(), Some(JSON), None),
    )
    .await;
    assert_eq!(json.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(error_code(&json.body), PAYLOAD_TYPE_UNSUPPORTED_CODE);

    // A batch that can only be answered in JSON cannot be answered.
    let accept_json = send(
        &router,
        typed_request(Method::POST, "/rpc/batch", sealed.clone(), None, Some(JSON)),
    )
    .await;
    assert_eq!(accept_json.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(hits.get(), 0);

    // JSON preferred, CBOR acceptable: the batch answers in CBOR, sealed.
    let call = batch_call();
    let sealed = call.seal(&batch_frames(&["procedure.read"])).await;
    let answer = send(
        &router,
        typed_request(
            Method::POST,
            "/rpc/batch",
            sealed.clone(),
            None,
            Some("application/json, application/cbor"),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-accept"), CBOR, "narrowed to CBOR");
    assert_eq!(answer.seen("cratestack-payload-type"), CBOR);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies as CBOR");
}
