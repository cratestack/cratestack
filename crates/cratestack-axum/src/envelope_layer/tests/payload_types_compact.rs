//! Compact payload types under the seal (`docs/design/compact-wire-keys.md`
//! §5.1): a route declares the named shape it speaks, so it permits a
//! compact type exactly when it permits the plain counterpart, provided
//! the layer opted in to the compact type itself.

use cratestack_core::compact::{COMPACT_CBOR, COMPACT_JSON};
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::payload_types_support::*;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

const BOTH: [&str; 4] = [CBOR, JSON, COMPACT_CBOR, COMPACT_JSON];

fn layer(types: &[&'static str], rpc: bool) -> EnvelopeLayer {
    let builder = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required);
    let builder = match rpc {
        true => builder.rpc(""),
        false => builder.rest("", &REST_ROUTES),
    };
    builder
        .payload_media_types(types.iter().copied(), types.iter().copied())
        .build()
        .expect("layer")
}

#[tokio::test]
async fn a_compact_type_is_permitted_where_its_plain_type_is_declared() {
    let hits = Hits::default();
    let router = rest_router(layer(&BOTH, false), &hits);
    let call = Call::new(Method::POST, "/widgets", &[]).types(COMPACT_CBOR, CBOR);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &router,
        typed_request(
            Method::POST,
            "/widgets",
            sealed.clone(),
            Some(COMPACT_CBOR),
            Some("application/vnd.cratestack.compact+cbor, application/cbor"),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-content-type"), COMPACT_CBOR);
    assert_eq!(
        answer.seen("x-seen-accept"),
        "application/vnd.cratestack.compact+cbor, application/cbor"
    );
    assert_eq!(answer.seen("x-seen-payload-type"), "<absent>");
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn a_compact_json_type_follows_a_route_that_declares_json() {
    let hits = Hits::default();
    let router = rest_router(layer(&BOTH, false), &hits);
    let call = Call::new(Method::POST, "/either", &[]).types(COMPACT_JSON, COMPACT_JSON);
    let sealed = call.seal(b"{}").await;
    let answer = send(
        &router,
        typed_request(
            Method::POST,
            "/either",
            sealed,
            Some(COMPACT_JSON),
            Some("application/json, application/vnd.cratestack.compact+json"),
        ),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.seen("x-seen-content-type"), COMPACT_JSON);
}

#[tokio::test]
async fn a_route_that_does_not_declare_the_plain_type_refuses_the_compact_one() {
    let hits = Hits::default();
    // `/pay` declares a form in and JSON out; it has no CBOR shape.
    let sealed = Call::new(Method::POST, "/pay", &[]).seal(PAYLOAD).await;
    let answer = send(
        &rest_router(layer(&BOTH, false), &hits),
        typed_request(Method::POST, "/pay", sealed, Some(COMPACT_CBOR), None),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_layer_that_did_not_opt_in_refuses_compact_unsigned() {
    let hits = Hits::default();
    let router = rest_router(layer(&[CBOR, JSON], false), &hits);
    let sealed = Call::new(Method::POST, "/widgets", &[]).seal(PAYLOAD).await;
    let request = send(
        &router,
        typed_request(
            Method::POST,
            "/widgets",
            sealed.clone(),
            Some(COMPACT_CBOR),
            None,
        ),
    )
    .await;
    assert_eq!(request.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!request.is_sealed());
    let accept = send(
        &router,
        typed_request(Method::POST, "/widgets", sealed, None, Some(COMPACT_CBOR)),
    )
    .await;
    assert_eq!(accept.status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn rpc_ops_follow_the_same_rule_and_batch_stays_plain_cbor() {
    let hits = Hits::default();
    let router = rpc_router(layer(&BOTH, true), &hits);
    let call = Call::new(Method::POST, "procedure.notify", &[]).types(COMPACT_CBOR, CBOR);
    let sealed = call.seal(PAYLOAD).await;
    let op = send(
        &router,
        typed_request(
            Method::POST,
            "/rpc/procedure.notify",
            sealed,
            Some(COMPACT_CBOR),
            None,
        ),
    )
    .await;
    assert_eq!(op.status, StatusCode::OK);
    assert_eq!(op.seen("x-seen-content-type"), COMPACT_CBOR);

    let batch = Call::new(Method::POST, "batch", &[])
        .seal(&batch_frames(&["procedure.read"]))
        .await;
    let refused = send(
        &router,
        typed_request(Method::POST, "/rpc/batch", batch, Some(COMPACT_CBOR), None),
    )
    .await;
    assert_eq!(refused.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}
