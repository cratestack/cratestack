//! RPC-specific rules: the op-id shape (decision B2), the CORS preflight,
//! a signed subscription refused before its handler runs, and the RPC
//! twins of the REST replay, body-cap and sealed-`500` tests.

use axum::Router;
use axum::extract::Path;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use cratestack_core::request_digest;
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn layer() -> EnvelopeLayer {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .build()
        .expect("layer")
}

#[tokio::test]
async fn a_percent_encoded_batch_is_never_bound_as_batch() {
    let hits = Hits::default();
    let router = rpc_router(layer(), &hits);
    let call = Call::new(Method::POST, "batch", &[]);
    let sealed = call.seal(&batch_frames(&["procedure.x"])).await;
    let answer = send(
        &router,
        cose_request(Method::POST, "/rpc/%62atch", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(!answer.is_sealed());
    assert_eq!(hits.get(), 0);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect_err("no signed answer a client could take for its batch");
    // Not an op, so plain traffic passes to the router (whose `404` it is
    // for a generated one), rather than failing closed as misconfigured.
    for path in ["/rpc/%62atch", "/rpc/undotted", "/rpc/a%2Fb.c"] {
        let plain = send(&router, plain_request(Method::POST, path, PAYLOAD)).await;
        assert_eq!(plain.status, StatusCode::OK, "{path}");
    }
    assert_eq!(hits.get(), 3);
}

/// A CORS layer inside the envelope answers preflights with a handler like
/// this one; a bodiless `OPTIONS` reaches it, one with a body does not.
#[tokio::test]
async fn a_bodiless_preflight_passes_but_an_options_with_a_body_does_not() {
    let hits = Hits::default();
    let counted = hits.clone();
    let preflight_handler = move || {
        counted.hit();
        async { StatusCode::NO_CONTENT }
    };
    let router = Router::new()
        .route(
            "/rpc/{op_id}",
            post(|| async { StatusCode::OK }).options(preflight_handler),
        )
        .layer(layer());
    let preflight = send(
        &router,
        plain_request(Method::OPTIONS, "/rpc/procedure.x", b""),
    )
    .await;
    assert_eq!(preflight.status, StatusCode::NO_CONTENT);
    assert!(!preflight.is_sealed());
    assert_eq!(hits.get(), 1);
    let with_body = send(
        &router,
        plain_request(Method::OPTIONS, "/rpc/procedure.x", PAYLOAD),
    )
    .await;
    assert_eq!(
        with_body.status,
        StatusCode::METHOD_NOT_ALLOWED,
        "the layer's own"
    );
    // The RPC binding's stable vocabulary has no wrong-method code.
    assert_eq!(error_code(&with_body.body), "invalid_argument");
    assert_eq!(hits.get(), 1, "not waved through to the handler");
}

#[tokio::test]
async fn a_signed_subscription_is_a_sealed_406_before_its_handler_runs() {
    for mode in [EnvelopeMode::Required, EnvelopeMode::Optional] {
        let hits = Hits::default();
        let counted = hits.clone();
        let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
            .policy(mode)
            .rpc("")
            .build()
            .expect("layer");
        let handler = move || {
            counted.hit();
            async { StatusCode::NO_CONTENT }
        };
        let router = Router::new()
            .route("/rpc/subscribe/{op_id}", get(handler))
            .layer(layer);
        let call = Call::new(Method::GET, "subscribe/model.Widget.subscribe", &[]);
        let sealed = call.seal(&[]).await;
        let path = "/rpc/subscribe/model.Widget.subscribe";
        let answer = send(&router, cose_request(Method::GET, path, sealed.clone())).await;
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE, "{mode:?}");
        assert_eq!(hits.get(), 0, "{mode:?}: the handler must not run");
        let body = call
            .open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("sealed");
        assert_eq!(error_code(&body), "invalid_argument", "RPC vocabulary");
    }
}

#[tokio::test]
async fn a_replayed_call_is_refused() {
    let hits = Hits::default();
    let router = rpc_router(layer(), &hits);
    let sealed = Call::new(Method::POST, "procedure.notify", &[])
        .seal(PAYLOAD)
        .await;
    let path = "/rpc/procedure.notify";
    let first = send(&router, cose_request(Method::POST, path, sealed.clone())).await;
    assert_eq!(first.status, StatusCode::OK);
    let replay = send(&router, cose_request(Method::POST, path, sealed)).await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    assert!(!replay.is_sealed());
    assert_eq!(error_code(&replay.body), "unauthenticated");
    assert_eq!(hits.get(), 1);
}

#[tokio::test]
async fn a_call_over_the_layers_cap_is_the_rpc_413_unsigned() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Required)
        .rpc("")
        .max_body_bytes(8)
        .build()
        .expect("layer");
    let sealed = Call::new(Method::POST, "procedure.notify", &[])
        .seal(PAYLOAD)
        .await;
    let answer = send(
        &rpc_router(layer, &hits),
        cose_request(Method::POST, "/rpc/procedure.notify", sealed),
    )
    .await;
    assert_eq!(answer.status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(!answer.is_sealed());
    assert_eq!(error_code(&answer.body), "invalid_argument");
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn a_non_cbor_answer_to_a_signed_call_is_a_sealed_rpc_error() {
    let odd = |Path(op): Path<String>| async move {
        if op == "procedure.json" {
            ([(header::CONTENT_TYPE, "application/json")], "{}").into_response()
        } else {
            let headers = [(header::CONTENT_TYPE, "text/plain")];
            (StatusCode::TOO_MANY_REQUESTS, headers, "slow down").into_response()
        }
    };
    let router = Router::new()
        .route("/rpc/{op_id}", post(odd))
        .layer(layer());
    for (op, status, code) in [
        (
            "procedure.json",
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
        ),
        (
            "procedure.busy",
            StatusCode::TOO_MANY_REQUESTS,
            "resource_exhausted",
        ),
    ] {
        let call = Call::new(Method::POST, op, &[]);
        let sealed = call.seal(PAYLOAD).await;
        let path = format!("/rpc/{op}");
        let answer = send(&router, cose_request(Method::POST, &path, sealed.clone())).await;
        assert_eq!(answer.status, status, "{op}");
        let body = call
            .open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("sealed");
        assert_eq!(error_code(&body), code, "{op}: RpcErrorBody vocabulary");
    }
}
