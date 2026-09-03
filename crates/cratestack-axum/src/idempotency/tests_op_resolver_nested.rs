//! Resolver behaviour under `Router::nest` (#881).
//!
//! Split from the sibling `tests_op_resolver.rs` for the 200-line
//! ceiling; the descriptor fixtures are shared from there, so the
//! root-mounted and nested cases cannot drift on what the schema is
//! supposed to declare.
//!
//! The bug these pin was measured, not hypothesised: this crate's own
//! README mounts the generated router with `.nest("/api", router)`,
//! and under that mount the plain constructors miss every lookup —
//! safe, because a miss reserves, but `@no_idempotency` becomes inert.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::{MatchedPath, Request};
use axum::http::{Request as HttpRequest, StatusCode};
use axum::routing::post;
use tower::ServiceExt;

use super::tests_op_resolver::{OPS, ROUTES, post_request};
use super::{
    build_rest_op_resolver, build_rest_op_resolver_with_prefix, build_rpc_op_resolver,
    build_rpc_op_resolver_with_prefix,
};

type Resolver = Box<dyn Fn(&Request) -> cratestack_exec::OpAdmission + Send + Sync>;

/// Drive a request through a router nested under `/api` and report what
/// `resolver` made of it.
///
/// A real nested router is the only way to get this right: `MatchedPath`
/// reports the full path *including* the nest prefix, and that is exactly
/// the fact the plain constructor gets wrong.
async fn nested_resolves_to_bypass(uri: &str, resolver: Resolver) -> bool {
    let resolver = Arc::new(resolver);
    let captured: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    let handler_captured = captured.clone();
    let handler = move |matched: MatchedPath, mut req: Request| {
        let resolver = resolver.clone();
        let captured = handler_captured.clone();
        async move {
            req.extensions_mut().insert(matched);
            *captured.lock().unwrap() = Some(resolver(&req).idempotent_by_default);
            "ok"
        }
    };

    let inner = Router::new()
        .route("/$procs/createPayment", post(handler.clone()))
        .route("/widgets", post(handler));

    let response = Router::new()
        .nest("/api", inner)
        .oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router is infallible");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the probe route must actually match, or this test proves nothing"
    );

    let flag = *captured.lock().unwrap();
    flag.expect("probe handler ran")
}

/// The measured bug: under `Router::nest`, the plain constructor's exact
/// comparison misses every route, so a `@no_idempotency` procedure keeps
/// reserving. Safe (a miss reserves) but silently inert — which is the
/// failure mode #876 set out to end.
#[tokio::test]
async fn nested_mount_without_a_prefix_reserves_everything() {
    assert!(
        !nested_resolves_to_bypass(
            "/api/$procs/createPayment",
            Box::new(build_rest_op_resolver(ROUTES))
        )
        .await,
        "without the mount prefix the descriptor lookup misses, and a miss must \
         reserve — the feature is inert here, never unsafe"
    );
}

/// With the prefix supplied, the same request resolves and the annotated
/// procedure bypasses.
#[tokio::test]
async fn nested_mount_with_the_prefix_resolves_the_annotated_procedure() {
    assert!(
        nested_resolves_to_bypass(
            "/api/$procs/createPayment",
            Box::new(build_rest_op_resolver_with_prefix("/api", ROUTES))
        )
        .await,
        "with the mount prefix supplied, a @no_idempotency procedure under a \
         nested router must bypass"
    );
}

/// Negative control: the prefix must not turn every nested route into a
/// bypass, only the ones the schema marked.
#[tokio::test]
async fn nested_mount_with_the_prefix_still_reserves_ordinary_routes() {
    assert!(
        !nested_resolves_to_bypass(
            "/api/widgets",
            Box::new(build_rest_op_resolver_with_prefix("/api", ROUTES))
        )
        .await,
        "a model write under a nested router must still reserve"
    );
}

#[test]
fn rpc_prefix_resolver_reads_the_flag_under_a_nested_mount() {
    let plain = build_rpc_op_resolver(OPS);
    let prefixed = build_rpc_op_resolver_with_prefix("/api", OPS);

    assert!(
        !plain(&post_request("/api/rpc/procedure.createPayment")).idempotent_by_default,
        "without the prefix the `/rpc/` test fails and everything reserves"
    );
    assert!(
        prefixed(&post_request("/api/rpc/procedure.createPayment")).idempotent_by_default,
        "with the prefix the annotated op bypasses"
    );
    assert!(
        !prefixed(&post_request("/api/rpc/procedure.transfer")).idempotent_by_default,
        "an ordinary mutation under the same mount still reserves"
    );
}

/// A prefixed resolver must not be fooled by a path that merely starts
/// with the same characters, and must keep every framework dispatch point
/// reserving.
#[test]
fn rpc_prefix_resolver_keeps_every_fail_closed_case() {
    let prefixed = build_rpc_op_resolver_with_prefix("/api", OPS);

    for uri in [
        "/apiary/rpc/procedure.createPayment",
        "/rpc/procedure.createPayment",
        "/api/rpc/batch",
        "/api/rpc/subscribe/model.Widget.subscribe",
        "/api/rpc/procedure.doesNotExist",
        "/api/widgets",
    ] {
        assert!(
            !prefixed(&post_request(uri)).idempotent_by_default,
            "{uri} must fail closed toward RESERVING under a prefixed resolver too"
        );
    }
}
