//! The REST half of a signed open (cratestack#1378): the table row is
//! `"<METHOD> <path>"`, matched on the method as sent. A route sharing the
//! path under another method, or a `HEAD`, is not the marked op.

use axum::Router;
use axum::extract::Request;
use axum::routing::get;
use cratestack_core::{
    FramesMode, RouteTransportCapabilities, RouteTransportDescriptor, SubscriptionStream,
    request_digest,
};
use futures_util::stream;
use http::{HeaderValue, Method, StatusCode, header};

use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};
use crate::rpc::{encode_subscribe_error, negotiate_subscribe_binding, serve_subscription_with};
use crate::transport::StreamedResponseMarker;

const PATH: &str = "/$procs/hinted";
const CAPABILITIES: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &["application/cbor"],
    response_types: &["application/cbor", "application/cbor-seq"],
    default_response_type: "application/cbor",
    supports_sequence_response: true,
};
static ROUTES: [RouteTransportDescriptor; 2] = [
    RouteTransportDescriptor {
        name: "hinted",
        method: "GET",
        path: PATH,
        capabilities: CAPABILITIES,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    },
    RouteTransportDescriptor {
        name: "hinted_post",
        method: "POST",
        path: PATH,
        capabilities: CAPABILITIES,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    },
];
static CONTRACTS_REST: cratestack_core::AcceptedContracts = &[
    ("GET /$procs/hinted", &[CONTRACT]),
    ("POST /$procs/hinted", &[CONTRACT]),
];
const TABLE: &[(&str, FramesMode)] = &[("GET /$procs/hinted", FramesMode::Unsigned)];
const ASKS: &str = "application/cbor-seq, application/cose; cose-type=\"cose-sign1\"";

async fn open_get(req: Request) -> axum::response::Response {
    let binding = match negotiate_subscribe_binding(req.headers()) {
        Ok(binding) => binding,
        Err(error) => return encode_subscribe_error(&error),
    };
    let items: SubscriptionStream<serde_json::Value> =
        Box::pin(stream::iter([Ok(serde_json::json!({"n": 1}))]));
    serve_subscription_with(binding, req.extensions(), async { Ok(items) }).await
}

fn plain_stream() -> axum::response::Response {
    let mut response = axum::response::Response::new(axum::body::Body::from("\u{1}"));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/cbor-seq"),
    );
    response.extensions_mut().insert(StreamedResponseMarker);
    response
}

fn router(hits: &Hits) -> Router {
    let counted = hits.clone();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS_REST)
        .policy(EnvelopeMode::Required)
        .rest("", &ROUTES)
        .subscriptions(TABLE)
        .build()
        .expect("layer");
    Router::new()
        .route(
            PATH,
            get(open_get).post(move || {
                counted.hit();
                async { plain_stream() }
            }),
        )
        .layer(layer)
}

async fn sealed_call(method: Method) -> (Call, bytes::Bytes, Request) {
    let call = Call::new(method.clone(), PATH, &[]);
    let sealed = call.seal(&[]).await;
    let mut request = cose_request(method, PATH, sealed.clone());
    request
        .headers_mut()
        .insert(header::ACCEPT, HeaderValue::from_static(ASKS));
    (call, sealed, request)
}

#[tokio::test]
async fn a_marked_rest_get_streams_unsigned_frames() {
    let router = router(&Hits::default());
    let (.., request) = sealed_call(Method::GET).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(answer.content_type(), "application/cbor-seq");
    assert_eq!(answer.seen("cratestack-frames"), "unsigned");
    assert_eq!(answer.seen("cache-control"), "no-store");
    assert!(!answer.is_sealed());
}

/// The same path under POST has no row: not a marked open, so its stream is
/// swapped for a sealed 406 (the handler runs first on REST).
#[tokio::test]
async fn another_method_on_a_marked_path_is_not_the_marked_op() {
    let hits = Hits::default();
    let router = router(&hits);
    let (call, sealed, request) = sealed_call(Method::POST).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
    assert_eq!(hits.get(), 1);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("sealed");
}

/// axum serves `HEAD` from the `GET` route, but a `HEAD` is never the row.
#[tokio::test]
async fn a_sealed_head_is_never_the_marked_op() {
    let router = router(&Hits::default());
    let (_call, _sealed, request) = sealed_call(Method::HEAD).await;
    let answer = send(&router, request).await;
    assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
    assert!(answer.is_sealed(), "{}", answer.content_type());
    assert_eq!(answer.seen("cratestack-frames"), "<absent>");
}
