//! `RateLimitLayer::with_op_resolver` (ADR 0015 slice 2, cratestack#877):
//! the layer asks `OpExecutor` whether an op is limited, using the same
//! resolvers idempotency uses — which is what makes `@no_rate_limit`
//! reachable under `Router::nest`, where the `build_*_ops_filter`
//! predicates cannot see it.
//!
//! Driven through a real nested router, not a hand-built `Request`: under
//! `nest` the path the layer sees carries the mount prefix, and that fact
//! is the whole subject.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode};
use axum::routing::post;
use cratestack_core::{OpDescriptor, OpKind};
use tower::ServiceExt;

use crate::idempotency::build_rpc_op_resolver_with_prefix;
use crate::ratelimit::{
    InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer, build_rpc_ops_filter,
};

const OPS: &[OpDescriptor] = &[
    OpDescriptor {
        op_id: "procedure.createPayment",
        kind: OpKind::Unary,
        input_ty: "PingArgs",
        output_ty: "PingArgs",
        idempotent_by_default: false,
        rate_limited_by_default: false,
        auth_required: true,
    },
    OpDescriptor {
        op_id: "procedure.ping",
        kind: OpKind::Unary,
        input_ty: "PingArgs",
        output_ty: "PingArgs",
        idempotent_by_default: true,
        rate_limited_by_default: true,
        auth_required: true,
    },
];

async fn ok() -> &'static str {
    "ok"
}

/// The generated RPC routes mounted the way this crate's README mounts
/// them, with a burst of one so the second charged call throttles.
fn nested(layer: impl FnOnce(RateLimitLayer) -> RateLimitLayer) -> Router {
    let inner = Router::new()
        .route("/rpc/procedure.createPayment", post(ok))
        .route("/rpc/procedure.ping", post(ok));
    let limiter = RateLimitLayer::new(
        Arc::new(InMemoryRateLimitStore::default()),
        RateLimitConfig::new(1, 0.001),
    );
    Router::new().nest("/api", inner).layer(layer(limiter))
}

/// `peer: None` sends a caller with no identity at all, which the default
/// key derivation refuses with `412` (cratestack#416) — so a 200 for it
/// proves the layer never derived a key.
async fn status(router: &Router, uri: &str, peer: Option<&str>) -> StatusCode {
    let mut request = Request::post(uri)
        .body(Body::empty())
        .expect("request should build");
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

#[tokio::test]
async fn prefixed_resolver_exempts_a_no_rate_limit_op_under_nest() {
    let router =
        nested(|layer| layer.with_op_resolver(build_rpc_op_resolver_with_prefix("/api", OPS)));

    for attempt in 0..3 {
        assert_eq!(
            status(&router, "/api/rpc/procedure.createPayment", None).await,
            StatusCode::OK,
            "attempt {attempt}: a @no_rate_limit op must pass past the burst, and \
             without a caller identity — exempt ops never derive a bucket key"
        );
    }
}

/// Why `with_op_resolver` exists: the RPC predicate reads the raw path,
/// sees `/api/rpc/...`, fails its `/rpc/` test and rate-limits — safe, but
/// `@no_rate_limit` is inert, and an identity-less caller is refused.
#[tokio::test]
async fn the_ops_filter_cannot_see_through_nest() {
    let router = nested(|layer| layer.with_should_rate_limit_fn(build_rpc_ops_filter(OPS)));

    assert_eq!(
        status(&router, "/api/rpc/procedure.createPayment", None).await,
        StatusCode::PRECONDITION_FAILED,
        "under nest the predicate cannot identify the op, treats it as limited, \
         and so derives a key the caller cannot supply"
    );
}

#[tokio::test]
async fn prefixed_resolver_still_throttles_an_ordinary_op() {
    let router =
        nested(|layer| layer.with_op_resolver(build_rpc_op_resolver_with_prefix("/api", OPS)));

    let peer = Some("192.0.2.60:1");
    assert_eq!(
        status(&router, "/api/rpc/procedure.ping", peer).await,
        StatusCode::OK
    );
    assert_eq!(
        status(&router, "/api/rpc/procedure.ping", peer).await,
        StatusCode::TOO_MANY_REQUESTS,
        "the prefix must not exempt ops the schema did not mark"
    );
}

/// A mount prefix read at runtime — config, env — must be installable:
/// the resolver owns its copy, so the source string can be dropped before
/// the first request. Before `use<>` on the builder this was E0597.
#[tokio::test]
async fn a_runtime_prefix_string_can_build_the_resolver() {
    let prefix = String::from("/api");
    let resolver = build_rpc_op_resolver_with_prefix(&prefix, OPS);
    drop(prefix);
    let router = nested(|layer| layer.with_op_resolver(resolver));

    assert_eq!(
        status(&router, "/api/rpc/procedure.createPayment", None).await,
        StatusCode::OK
    );
}

/// FAIL DIRECTION at the transport: an op id the resolver does not know is
/// CHARGED. A miss that exempted would let any unknown path spend nothing.
#[tokio::test]
async fn an_unknown_op_is_charged_not_exempted() {
    let router =
        nested(|layer| layer.with_op_resolver(build_rpc_op_resolver_with_prefix("/api", OPS)));

    let peer = Some("192.0.2.61:1");
    assert_eq!(
        status(&router, "/api/rpc/procedure.nope", peer).await,
        StatusCode::NOT_FOUND,
        "first unknown call is within the burst and reaches the 404 fallback"
    );
    assert_eq!(
        status(&router, "/api/rpc/procedure.nope", peer).await,
        StatusCode::TOO_MANY_REQUESTS,
        "an unresolved op is rate limited — the second call is throttled"
    );
}
