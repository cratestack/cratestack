//! REST twin of `tests_op_resolver` (transport parity): `@no_rate_limit`
//! under `Router::nest`, resolved through `MatchedPath` by
//! `build_rest_op_resolver_with_prefix` rather than the raw RPC path.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use cratestack_core::{RouteTransportCapabilities, RouteTransportDescriptor};
use tower::ServiceExt;

use crate::idempotency::build_rest_op_resolver_with_prefix;
use crate::ratelimit::{
    InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer, build_rest_ops_filter,
};

const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &[],
    response_types: &[],
    default_response_type: "",
    supports_sequence_response: false,
};

const ROUTES: &[RouteTransportDescriptor] = &[
    RouteTransportDescriptor {
        name: "createPayment",
        method: "POST",
        path: "/$procs/createPayment",
        capabilities: CAPS,
        idempotent_by_default: false,
        rate_limited_by_default: false,
    },
    RouteTransportDescriptor {
        name: "Widget",
        method: "GET",
        path: "/widgets/{id}",
        capabilities: CAPS,
        idempotent_by_default: true,
        rate_limited_by_default: true,
    },
];

async fn ok() -> &'static str {
    "ok"
}

fn nested(layer: impl FnOnce(RateLimitLayer) -> RateLimitLayer) -> Router {
    let inner = Router::new()
        .route("/$procs/createPayment", post(ok))
        .route("/widgets/{id}", get(ok));
    let limiter = RateLimitLayer::new(
        Arc::new(InMemoryRateLimitStore::default()),
        RateLimitConfig::new(1, 0.001),
    );
    Router::new().nest("/api", inner).layer(layer(limiter))
}

/// `peer: None` is a caller the default key derivation refuses with `412`,
/// so a 200 for it proves no key was derived.
async fn status(router: &Router, request: Request<Body>, peer: Option<&str>) -> StatusCode {
    let mut request = request;
    if let Some(peer) = peer {
        let peer: std::net::SocketAddr = peer.parse().expect("test peer parses");
        request.extensions_mut().insert(ConnectInfo(peer));
    }
    router
        .clone()
        .oneshot(request)
        .await
        .expect("router is infallible")
        .status()
}

fn post_payment() -> Request<Body> {
    Request::post("/api/$procs/createPayment")
        .body(Body::empty())
        .expect("request should build")
}

fn get_widget(id: u32) -> Request<Body> {
    Request::get(format!("/api/widgets/{id}"))
        .body(Body::empty())
        .expect("request should build")
}

#[tokio::test]
async fn prefixed_rest_resolver_exempts_and_throttles_under_nest() {
    let router =
        nested(|layer| layer.with_op_resolver(build_rest_op_resolver_with_prefix("/api", ROUTES)));

    for attempt in 0..3 {
        assert_eq!(
            status(&router, post_payment(), None).await,
            StatusCode::OK,
            "attempt {attempt}: a @no_rate_limit route under nest must pass past the \
             burst without a caller identity"
        );
    }

    let peer = Some("192.0.2.70:1");
    assert_eq!(status(&router, get_widget(42), peer).await, StatusCode::OK);
    assert_eq!(
        status(&router, get_widget(7), peer).await,
        StatusCode::TOO_MANY_REQUESTS,
        "an ordinary route is throttled whatever the concrete id — MatchedPath, not \
         the request path, is what resolves"
    );
}

/// The REST filter misses under nest (`MatchedPath` carries `/api`), so the
/// exempt route is treated as limited and an identity-less caller is refused.
#[tokio::test]
async fn the_rest_ops_filter_cannot_see_through_nest() {
    let router = nested(|layer| layer.with_should_rate_limit_fn(build_rest_ops_filter(ROUTES)));

    assert_eq!(
        status(&router, post_payment(), None).await,
        StatusCode::PRECONDITION_FAILED
    );
}
