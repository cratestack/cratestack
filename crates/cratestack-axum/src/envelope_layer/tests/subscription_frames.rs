//! `subscriptions(..)` (cratestack#1378, part A) is inert: the layer stores
//! the generated `SUBSCRIPTION_FRAMES` table, defaults to empty, and no
//! request is decided by it yet. A signed subscription open is still the
//! sealed `406` before its handler runs, marked or not.

use axum::Router;
use axum::routing::get;
use cratestack_core::{
    FramesMode, RouteTransportCapabilities, RouteTransportDescriptor, request_digest,
};
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};
use crate::transport::StreamedResponseMarker;

const TABLE: &[(&str, FramesMode)] = &[("model.Widget.subscribe", FramesMode::Unsigned)];

fn builder() -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rpc("")
}

#[test]
fn the_table_defaults_to_empty_and_is_stored_when_given() {
    let none = builder().build().expect("layer");
    assert!(
        none.config.subscriptions.is_empty(),
        "fail closed by default"
    );
    let some = builder().subscriptions(TABLE).build().expect("layer");
    assert_eq!(some.config.subscriptions, TABLE);
}

#[tokio::test]
async fn a_marked_subscription_is_still_the_sealed_406_until_the_open_is_signed() {
    for layer in [
        builder().build().expect("layer"),
        builder().subscriptions(TABLE).build().expect("layer"),
    ] {
        let hits = Hits::default();
        let counted = hits.clone();
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
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
        assert_eq!(hits.get(), 0, "the handler must not run");
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("sealed");
    }
}

/// The REST row is method-qualified and `is_subscription()` is false on REST,
/// so the table cannot change a REST decision: a marked REST subscription is
/// still a stream the layer replaces with a sealed 406. Unlike RPC, the
/// handler does run first (REST has no subscribe route to refuse earlier);
/// it is the streamed response that is swapped out.
#[tokio::test]
async fn a_marked_rest_subscription_is_still_the_sealed_406_under_required() {
    static ROUTES: [RouteTransportDescriptor; 1] = [RouteTransportDescriptor {
        name: "hinted",
        method: "GET",
        path: "/$procs/hinted",
        capabilities: RouteTransportCapabilities {
            request_types: &[],
            response_types: &["application/cbor", "application/cbor-seq"],
            default_response_type: "application/cbor",
            supports_sequence_response: true,
        },
        idempotent_by_default: false,
        rate_limited_by_default: true,
    }];
    static REST_CONTRACTS: cratestack_core::AcceptedContracts =
        &[("GET /$procs/hinted", &[CONTRACT])];
    const REST_TABLE: &[(&str, FramesMode)] = &[("GET /$procs/hinted", FramesMode::Unsigned)];
    let rest = || {
        EnvelopeLayer::builder(server_envelope(), AUDIENCE, REST_CONTRACTS)
            .policy(EnvelopeMode::Required)
            .rest("", &ROUTES)
    };
    for layer in [
        rest().build().expect("layer"),
        rest().subscriptions(REST_TABLE).build().expect("layer"),
    ] {
        let hits = Hits::default();
        let counted = hits.clone();
        let handler = move || {
            counted.hit();
            async {
                let mut response = axum::response::Response::new(axum::body::Body::from("\u{1}"));
                response.headers_mut().insert(
                    http::header::CONTENT_TYPE,
                    http::HeaderValue::from_static("application/cbor-seq"),
                );
                response.extensions_mut().insert(StreamedResponseMarker);
                response
            }
        };
        let router = Router::new()
            .route("/$procs/hinted", get(handler))
            .layer(layer);
        let call = Call::new(Method::GET, "/$procs/hinted", &[]);
        let sealed = call.seal(&[]).await;
        let path = "/$procs/hinted";
        let answer = send(&router, cose_request(Method::GET, path, sealed.clone())).await;
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
        assert!(answer.is_sealed(), "{}", answer.content_type());
        assert_eq!(
            hits.get(),
            1,
            "REST runs the handler, then swaps the stream"
        );
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("sealed");
    }
}
