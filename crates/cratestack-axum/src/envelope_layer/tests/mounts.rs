//! Nested routers: the binding never includes the mount prefix, a
//! parameterised mount's values are bound, an unmatched path passes
//! through (D6) unless it carries a COSE body, and under `Required` a
//! matched route the resolver cannot bind fails closed unless allow-listed
//! (decision S2).

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
async fn without_the_prefix_nothing_resolves_and_every_request_is_refused() {
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
    // The misconfiguration's other half: before decision S2 plain traffic
    // to every op passed unsigned here. Under `Required` it now fails
    // closed, unsigned (no op to bind a seal to), and the handler never runs.
    let plain = send(&app, plain_request(Method::GET, "/api/widgets/7", b"")).await;
    assert_eq!(plain.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!plain.is_sealed());
    assert_eq!(hits.get(), 0);
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
    // A route the router has but the descriptors do not list: under
    // `Required`, refused unless allow-listed (S2).
    let unlisted = send(&app, plain_request(Method::GET, "/unlisted", b"")).await;
    assert_eq!(unlisted.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(hits.get(), 0);
}

#[tokio::test]
async fn an_allow_listed_hand_written_route_passes_plain_but_never_cose() {
    let hits = Hits::default();
    let layer = builder()
        .rest("", &REST_ROUTES)
        .allow_unresolved(["/unlisted"])
        .build()
        .expect("layer");
    let app = rest_router(layer, &hits);
    let unlisted = send(&app, plain_request(Method::GET, "/unlisted", b"")).await;
    assert_eq!(unlisted.status, StatusCode::OK);
    assert!(!unlisted.is_sealed());
    let sealed = Call::new(Method::GET, "/unlisted", &[]).seal(&[]).await;
    let cose = send(&app, cose_request(Method::GET, "/unlisted", sealed)).await;
    assert_eq!(cose.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    // The allow-list names that route, not the generated ones.
    let op = send(&app, plain_request(Method::GET, "/widgets/1", b"")).await;
    assert_eq!(op.status, StatusCode::UNAUTHORIZED);
    assert_eq!(hits.get(), 1);
}

/// Under `Optional` (or `Off`) the policy's `unresolved_mode` is not
/// `Required`: a matched route nobody resolved passes, as D6 had it.
#[tokio::test]
async fn under_optional_an_unresolved_route_still_passes() {
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(EnvelopeMode::Optional)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let app = rest_router(layer, &hits);
    let unlisted = send(&app, plain_request(Method::GET, "/unlisted", b"")).await;
    assert_eq!(unlisted.status, StatusCode::OK);
}
