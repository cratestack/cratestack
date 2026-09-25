//! Nested routers: the binding never includes the mount prefix, a
//! parameterised mount's values are bound, and a path the generated router
//! does not serve passes through (D6), unless it carries a COSE body.

use axum::Router;
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode};

fn builder() -> crate::envelope_layer::EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA).policy(EnvelopeMode::Required)
}

#[tokio::test]
async fn a_nested_router_with_its_prefix_configured_binds_the_schema_paths() {
    let hits = Hits::default();
    let rest = rest_router(
        builder().rest("/api", &REST_ROUTES).build().expect("layer"),
        &hits,
    );
    let rpc = rpc_router(builder().rpc("/api/").build().expect("layer"), &hits);
    let app = Router::new().nest("/api", rest.merge(rpc));

    let call = Call::new(Method::GET, "/widgets/{id}", &["7"]);
    let sealed = call.seal(&[]).await;
    let answer = send(
        &app,
        cose_request(Method::GET, "/api/widgets/7", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");

    let call = Call::new(Method::POST, "procedure.notify", &[]);
    let sealed = call.seal(PAYLOAD).await;
    let answer = send(
        &app,
        cose_request(Method::POST, "/api/rpc/procedure.notify", sealed.clone()),
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
}

#[tokio::test]
async fn without_the_prefix_nothing_resolves_and_cose_bodies_are_refused() {
    let hits = Hits::default();
    let rest = rest_router(
        builder().rest("", &REST_ROUTES).build().expect("layer"),
        &hits,
    );
    let app = Router::new().nest("/api", rest);

    let sealed = Call::new(Method::GET, "/widgets/{id}", &["7"])
        .seal(&[])
        .await;
    let answer = send(&app, cose_request(Method::GET, "/api/widgets/7", sealed)).await;
    assert_eq!(answer.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(hits.get(), 0);
    // The misconfiguration's other half, and why the prefix matters: an
    // unresolved route is "not a generated op" (D6), so plain traffic to it
    // is not refused. The layer logs a warning once per process.
    let plain = send(&app, plain_request(Method::GET, "/api/widgets/7", b"")).await;
    assert_eq!(plain.status, StatusCode::OK);
}

#[tokio::test]
async fn a_parameterised_mount_binds_its_values_too() {
    let hits = Hits::default();
    let rest = rest_router(
        builder()
            .rest("/t/{tenant}", &REST_ROUTES)
            .build()
            .expect("layer"),
        &hits,
    );
    let app = Router::new().nest("/t/{tenant}", rest);

    let call = Call::new(Method::GET, "/widgets/{id}", &["acme", "1"]);
    let sealed = call.seal(&[]).await;
    let answer = send(&app, cose_request(Method::GET, "/t/acme/widgets/1", sealed)).await;
    assert_eq!(answer.status, StatusCode::OK);

    let sealed = call.seal(&[]).await;
    let other = send(
        &app,
        cose_request(Method::GET, "/t/globex/widgets/1", sealed),
    )
    .await;
    assert_eq!(
        other.status,
        StatusCode::UNAUTHORIZED,
        "tenant acme's request at globex"
    );
}

#[tokio::test]
async fn an_unmatched_path_passes_through_unless_it_carries_cose() {
    let hits = Hits::default();
    let app = rest_router(
        builder().rest("", &REST_ROUTES).build().expect("layer"),
        &hits,
    );
    let plain = send(&app, plain_request(Method::GET, "/nowhere", b"")).await;
    assert_eq!(plain.status, StatusCode::NOT_FOUND);
    assert!(!plain.is_sealed());
    // A route the router has but the descriptors do not list.
    let unlisted = send(&app, plain_request(Method::GET, "/unlisted", b"")).await;
    assert_eq!(unlisted.status, StatusCode::OK);
    let sealed = Call::new(Method::GET, "/unlisted", &[]).seal(&[]).await;
    let cose = send(&app, cose_request(Method::GET, "/unlisted", sealed)).await;
    assert_eq!(cose.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}
