//! Decision S2: under a `Required` policy, a route the router matched but
//! the resolver cannot bind fails closed (unsigned `500`, "envelope
//! misconfigured"), unless allow-listed; a generated path with another
//! method is the layer's own `405`. What a wrong or missing mount prefix,
//! `nest_service`, or descriptor drift looks like.

use axum::Router;
use http::{Method, StatusCode, header};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode, PolicyRequest};

fn required() -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA).policy(EnvelopeMode::Required)
}

async fn assert_misconfigured(router: &Router, method: Method, path: &str, hits: &Hits) {
    let answer = send(router, plain_request(method, path, b"")).await;
    assert_eq!(answer.status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
    assert!(
        !answer.is_sealed(),
        "{path}: no op, nothing to seal against"
    );
    assert_eq!(hits.get(), 0, "{path}: the handler must not run");
}

#[tokio::test]
async fn a_wrong_prefix_fails_closed_on_both_transports() {
    let hits = Hits::default();
    let rest = rest_router(
        required()
            .rest("/wrong", &REST_ROUTES)
            .build()
            .expect("layer"),
        &hits,
    );
    assert_misconfigured(&rest, Method::GET, "/widgets/1", &hits).await;
    let rpc = rpc_router(required().rpc("/wrong").build().expect("layer"), &hits);
    assert_misconfigured(&rpc, Method::POST, "/rpc/procedure.notify", &hits).await;
}

#[tokio::test]
async fn nest_service_without_the_prefix_fails_closed() {
    let hits = Hits::default();
    let layer = required().rest("", &REST_ROUTES).build().expect("layer");
    let app = Router::new().nest_service("/api", rest_router(layer, &hits));
    let plain = send(&app, plain_request(Method::GET, "/api/widgets/7", b"")).await;
    assert_ne!(plain.status, StatusCode::OK);
    assert_eq!(hits.get(), 0);
}

/// The class the API review's `@api_version` finding belonged to: the
/// descriptors name one path, the router serves another.
#[tokio::test]
async fn descriptor_drift_fails_closed() {
    let mut drifted = REST_ROUTES[1];
    drifted.path = "/v1/widgets/{id}";
    let routes: &'static [_] = Box::leak(Box::new([drifted]));
    let hits = Hits::default();
    let router = rest_router(required().rest("", routes).build().expect("layer"), &hits);
    assert_misconfigured(&router, Method::GET, "/widgets/1", &hits).await;
}

#[tokio::test]
async fn a_closure_policy_fails_closed_by_default() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(|_: &PolicyRequest<'_>| EnvelopeMode::Optional)
        .rest("/wrong", &REST_ROUTES)
        .build()
        .expect("layer");
    let router = rest_router(layer, &hits);
    assert_misconfigured(&router, Method::GET, "/widgets/1", &hits).await;
}

#[tokio::test]
async fn another_method_on_a_generated_path_is_the_layers_own_405() {
    let hits = Hits::default();
    let router = rest_router(
        required().rest("", &REST_ROUTES).build().expect("layer"),
        &hits,
    );
    let answer = send(&router, plain_request(Method::PUT, "/widgets/1", b"")).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);
    assert!(!answer.is_sealed());
    let allow = answer.headers.get(header::ALLOW).expect("Allow");
    assert_eq!(allow, "GET, DELETE");
    // The body names the status (second-review nit), not `BAD_REQUEST`.
    assert_eq!(error_code(&answer.body), "METHOD_NOT_ALLOWED");
    assert_eq!(hits.get(), 0);
}

/// A hand-written method on a generated path runs only when allow-listed;
/// without it the layer answers the `405` itself.
#[tokio::test]
async fn a_hand_written_method_on_a_generated_path_needs_the_allow_list() {
    for (allowed, expected) in [
        (false, StatusCode::METHOD_NOT_ALLOWED),
        (true, StatusCode::OK),
    ] {
        let mut builder = required().rest("", &REST_ROUTES);
        if allowed {
            builder = builder.allow_unresolved(["/widgets/{id}"]);
        }
        let layer = builder.build().expect("layer");
        let router = Router::new()
            .route(
                "/widgets/{id}",
                axum::routing::put(|| async { StatusCode::OK }),
            )
            .layer(layer);
        let answer = send(&router, plain_request(Method::PUT, "/widgets/1", b"")).await;
        assert_eq!(answer.status, expected, "allow-listed: {allowed}");
    }
}

#[test]
fn an_allow_list_entry_must_be_a_template() {
    let built = required()
        .rest("", &REST_ROUTES)
        .allow_unresolved(["health"])
        .build();
    assert!(built.is_err());
}
