//! The two op resolvers, and the direction they fail in.
//!
//! Nested-mount coverage lives in the sibling
//! `tests_op_resolver_nested.rs`; the fixtures below are shared with
//! it rather than copied.
//!
//! Every assertion here is the **inverse** of what
//! `ratelimit/{rest,rpc}_ops_filter.rs`'s tests assert: a lookup miss
//! must resolve to something that RESERVES. Getting this backwards is
//! the failure mode #876's Risks section names — it would silently drop
//! reservations on every path a resolver could not identify, and no
//! existing test would notice, because they all identify theirs.

use axum::Router;
use axum::body::Body;
use axum::extract::{MatchedPath, Request};
use axum::http::Request as HttpRequest;
use axum::routing::{get, post};
use cratestack_core::{OpDescriptor, OpKind, RouteTransportCapabilities, RouteTransportDescriptor};
use tower::ServiceExt;

use super::{build_rest_op_resolver, build_rpc_op_resolver};

pub(super) const CAPS: RouteTransportCapabilities = RouteTransportCapabilities {
    request_types: &[],
    response_types: &[],
    default_response_type: "",
    supports_sequence_response: false,
};

pub(super) const ROUTES: &[RouteTransportDescriptor] = &[
    RouteTransportDescriptor {
        name: "createPayment",
        method: "POST",
        path: "/$procs/createPayment",
        capabilities: CAPS,
        idempotent_by_default: true,
        rate_limited_by_default: true,
    },
    RouteTransportDescriptor {
        name: "Widget",
        method: "POST",
        path: "/widgets",
        capabilities: CAPS,
        idempotent_by_default: false,
        rate_limited_by_default: true,
    },
];

pub(super) const OPS: &[OpDescriptor] = &[
    OpDescriptor {
        op_id: "procedure.createPayment",
        kind: OpKind::Unary,
        input_ty: "PingArgs",
        output_ty: "PingArgs",
        idempotent_by_default: true,
        rate_limited_by_default: true,
        auth_required: true,
    },
    OpDescriptor {
        op_id: "procedure.transfer",
        kind: OpKind::Unary,
        input_ty: "PingArgs",
        output_ty: "PingArgs",
        idempotent_by_default: false,
        rate_limited_by_default: true,
        auth_required: true,
    },
];

pub(super) fn post_request(uri: &str) -> Request {
    HttpRequest::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .expect("request should build")
}

// ----------------------------------------------------------------- RPC

#[test]
fn rpc_resolver_reads_the_flag_off_the_matching_descriptor() {
    let resolve = build_rpc_op_resolver(OPS);

    assert!(
        resolve(&post_request("/rpc/procedure.createPayment")).idempotent_by_default,
        "a @no_idempotency op must resolve to a bypass"
    );
    assert!(
        !resolve(&post_request("/rpc/procedure.transfer")).idempotent_by_default,
        "an ordinary mutation must resolve to a reservation"
    );
}

#[test]
fn rpc_resolver_reserves_for_unknown_ops_batch_subscribe_and_non_rpc_paths() {
    let resolve = build_rpc_op_resolver(OPS);

    for uri in [
        "/rpc/procedure.doesNotExist",
        "/rpc/batch",
        "/rpc/subscribe/model.Widget.subscribe",
        "/api/widgets",
        "/rpc/",
    ] {
        assert!(
            !resolve(&post_request(uri)).idempotent_by_default,
            "{uri} must fail closed toward RESERVING — bypassing here would \
             silently drop duplicate-execution protection for every path the \
             resolver cannot identify"
        );
    }
}

// ---------------------------------------------------------------- REST

/// `MatchedPath` is populated by axum's router and never by a
/// hand-built `Request`, so the REST resolver has to run inside a real
/// router — the same reason `ratelimit/rest_ops_filter.rs`'s tests mount
/// one. The probe handler answers with the resolved flag so the
/// assertion can read it back off the response body.
async fn resolves_to_bypass(uri: &str, method: &str) -> bool {
    async fn probe(matched: MatchedPath, req: Request) -> String {
        let mut req = req;
        req.extensions_mut().insert(matched);
        let resolve = build_rest_op_resolver(ROUTES);
        resolve(&req).idempotent_by_default.to_string()
    }

    let router = Router::new()
        .route("/$procs/createPayment", post(probe))
        .route("/widgets", post(probe))
        .route("/widgets/{id}", get(probe));

    let response = router
        .oneshot(
            HttpRequest::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("router is infallible");
    let bytes = axum::body::to_bytes(response.into_body(), 64)
        .await
        .expect("probe body is tiny");
    String::from_utf8(bytes.to_vec()).expect("probe writes ascii") == "true"
}

#[tokio::test]
async fn rest_resolver_reads_the_flag_off_the_matched_route_pattern() {
    assert!(
        resolves_to_bypass("/$procs/createPayment", "POST").await,
        "a @no_idempotency procedure's route must resolve to a bypass"
    );
    assert!(
        !resolves_to_bypass("/widgets", "POST").await,
        "a model write must resolve to a reservation"
    );
}

#[tokio::test]
async fn rest_resolver_reserves_when_the_route_is_absent_from_the_descriptors() {
    // `/widgets/{id}` GET is mounted on the router but is not in
    // `ROUTES`; `/widgets` POST is in `ROUTES` under a different
    // pattern. A lookup that compared paths loosely, or ignored the
    // method, would wrongly match one of them.
    assert!(
        !resolves_to_bypass("/widgets/42", "GET").await,
        "a route absent from the descriptor slice must fail closed toward reserving"
    );
}

#[test]
fn rest_resolver_reserves_when_there_is_no_matched_path_at_all() {
    // A 404 never acquires `MatchedPath`. Built directly rather than
    // through a router because that absence is precisely the state under
    // test.
    let resolve = build_rest_op_resolver(ROUTES);
    assert!(
        !resolve(&post_request("/does/not/exist")).idempotent_by_default,
        "no MatchedPath means no identified op, which must still reserve"
    );
}
