//! Hand-built routers standing in for the generated ones: an echo handler
//! that reports, in `x-seen-*` response headers, what the envelope handed
//! it, plus the odd shapes a real router produces (a CBOR error, a
//! `text/plain` one, JSON, a stream).

// Shared with the COSE suites, which the `envelope` feature alone does
// not compile; what only they use is dead there.
#![cfg_attr(not(feature = "cose"), allow(dead_code))]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use axum::routing::{get, post};
use cratestack_core::{
    CratestackError, RouteTransportCapabilities, RouteTransportDescriptor, VerifiedSigner,
};
use http::{HeaderValue, StatusCode, header};

use crate::envelope_layer::EnvelopeLayer;
use crate::ratelimit::VerifiedPrincipal;
use crate::transport::StreamedResponseMarker;

const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &["application/cbor"],
    response_types: &["application/cbor"],
    default_response_type: "application/cbor",
    supports_sequence_response: false,
};

const fn route(method: &'static str, path: &'static str) -> RouteTransportDescriptor {
    RouteTransportDescriptor {
        name: path,
        method,
        path,
        capabilities: CAPS,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    }
}

pub static REST_ROUTES: [RouteTransportDescriptor; 6] = [
    route("POST", "/widgets"),
    route("GET", "/widgets/{id}"),
    route("DELETE", "/widgets/{id}"),
    route("GET", "/text-error"),
    route("GET", "/json"),
    route("GET", "/stream"),
];

/// Counts how often the router behind the layer actually ran.
#[derive(Clone, Default)]
pub struct Hits(Arc<AtomicUsize>);

impl Hits {
    pub fn get(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }

    pub fn hit(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn header_or_absent(req: &Request, name: header::HeaderName) -> HeaderValue {
    req.headers()
        .get(name)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("<absent>"))
}

/// Echoes the body as CBOR; `id = 404` answers with the transport's CBOR
/// `404` instead.
async fn echo(hits: Hits, req: Request) -> Response {
    hits.hit();
    let content_type = header_or_absent(&req, header::CONTENT_TYPE);
    let accept = header_or_absent(&req, header::ACCEPT);
    let principal = req
        .extensions()
        .get::<VerifiedPrincipal>()
        .map_or("<absent>".to_owned(), |principal| principal.0.clone());
    let signer = req
        .extensions()
        .get::<VerifiedSigner>()
        .map_or(-1, VerifiedSigner::alg);
    if req.uri().path().ends_with("/404") {
        let path = req.uri().path().to_owned();
        return crate::middleware_error::middleware_error_response(
            req.headers(),
            &path,
            CratestackError::NotFound("no such widget".to_owned()),
        );
    }
    let body = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .expect("body");
    let mut response = Response::new(Body::from(body));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/cbor"),
    );
    headers.insert("x-seen-content-type", content_type);
    headers.insert("x-seen-accept", accept);
    headers.insert(
        "x-seen-principal",
        HeaderValue::from_str(&principal).expect("hex"),
    );
    headers.insert("x-seen-signer-alg", HeaderValue::from(signer));
    response
}

fn with_type(status: StatusCode, content_type: &'static str, body: &'static str) -> Response {
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

fn streamed() -> Response {
    let mut response = with_type(StatusCode::OK, "application/cbor-seq", "\u{1}\u{2}");
    response.extensions_mut().insert(StreamedResponseMarker);
    response
}

/// A REST router with the layer applied as its last `.layer(..)`.
pub fn rest_router(layer: EnvelopeLayer, hits: &Hits) -> Router {
    let echo_route = |hits: Hits| move |req: Request| echo(hits.clone(), req);
    Router::new()
        .route("/widgets", post(echo_route(hits.clone())))
        .route(
            "/widgets/{id}",
            get(echo_route(hits.clone())).delete(echo_route(hits.clone())),
        )
        .route(
            "/text-error",
            get(|| async { with_type(StatusCode::PAYLOAD_TOO_LARGE, "text/plain", "too big") }),
        )
        .route(
            "/json",
            get(|| async { with_type(StatusCode::OK, "application/json", "{}") }),
        )
        .route("/stream", get(|| async { streamed() }))
        .route("/unlisted", get(echo_route(hits.clone())))
        .layer(layer)
}

/// A `transport rpc` router with the layer applied last.
pub fn rpc_router(layer: EnvelopeLayer, hits: &Hits) -> Router {
    let echo_route = |hits: Hits| move |req: Request| echo(hits.clone(), req);
    Router::new()
        .route("/rpc/batch", post(echo_route(hits.clone())))
        .route("/rpc/subscribe/{op_id}", get(|| async { streamed() }))
        .route("/rpc/{op_id}", post(echo_route(hits.clone())))
        .layer(layer)
}
