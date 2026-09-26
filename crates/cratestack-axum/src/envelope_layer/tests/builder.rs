//! The builder's nits from the second review: the calls commute (a
//! `mount_prefix` given before `rest`/`rpc` survives them), and
//! `unresolved_mode(..)` opts a closure policy out of failing closed.

use axum::Router;
use cratestack_cose::request_digest;
use http::{Method, StatusCode};

use super::fixtures::{Hits, REST_ROUTES, rest_router, rpc_router};

use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode, PolicyRequest};

fn required() -> crate::envelope_layer::EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA).policy(EnvelopeMode::Required)
}

/// `mount_prefix` first, then the transport with the root prefix the
/// generated `envelope_layer` passes: the explicit prefix wins whatever the
/// order, so a nested router still binds the schema's paths.
#[tokio::test]
async fn a_mount_prefix_survives_a_later_rest_or_rpc() {
    let hits = Hits::default();
    let rest = required()
        .mount_prefix("/api")
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let rpc = required()
        .mount_prefix("/api")
        .rpc("")
        .build()
        .expect("layer");
    let app = Router::new().nest(
        "/api",
        rest_router(rest, &hits).merge(rpc_router(rpc, &hits)),
    );

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
    let path = "/api/rpc/procedure.notify";
    let answer = send(&app, cose_request(Method::POST, path, sealed.clone())).await;
    assert_eq!(answer.status, StatusCode::OK);
    call.open(request_digest(&sealed), answer.status, answer.body)
        .await
        .expect("verifies");
    assert_eq!(hits.get(), 2);
}

/// A closure policy fails closed on unresolved traffic by default
/// (`fail_closed.rs`); `.unresolved_mode(..)` is the explicit opt-out, and
/// it does not touch the ops' own modes.
#[tokio::test]
async fn unresolved_mode_overrides_a_closure_policys_default() {
    let policy = |_: &PolicyRequest<'_>| EnvelopeMode::Required;
    let hits = Hits::default();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .unresolved_mode(EnvelopeMode::Optional)
        .policy(policy)
        .rest("", &REST_ROUTES)
        .build()
        .expect("layer");
    let counted = hits.clone();
    let router = Router::new()
        .route("/health", axum::routing::get(|| async { StatusCode::OK }))
        .route(
            "/widgets/{id}",
            axum::routing::get(move || {
                counted.hit();
                async { StatusCode::OK }
            }),
        )
        .layer(layer);
    let health = send(&router, plain_request(Method::GET, "/health", b"")).await;
    assert_eq!(
        health.status,
        StatusCode::OK,
        "unresolved: Optional, passes"
    );
    let op = send(&router, plain_request(Method::GET, "/widgets/1", b"")).await;
    assert_eq!(
        op.status,
        StatusCode::UNAUTHORIZED,
        "the op is still Required"
    );
    assert_eq!(hits.get(), 0);
}
