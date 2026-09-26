//! Security finding SF-2 (second review): axum answers `HEAD` with a `GET`
//! route, so `HEAD /rpc/subscribe/{op_id}` runs the subscription handler.
//! The RPC resolver used to answer it `MethodNotAllowed`, which a policy
//! whose `unresolved_mode` is not `Required` lets through: the handler ran
//! unsigned for an op the policy made `Required`. It now binds `HEAD`
//! wherever it expects `GET`, with the real method, as the REST resolver
//! does, so the op's own mode applies.

use axum::Router;
use axum::routing::get;
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeMode, EnvelopePolicy, PolicyRequest};

/// Every op `Required`; unresolved traffic `self.0` (a user with
/// hand-written routes who would rather not list them).
struct RequiredOps(EnvelopeMode);

impl EnvelopePolicy for RequiredOps {
    fn mode(&self, _request: &PolicyRequest<'_>) -> EnvelopeMode {
        EnvelopeMode::Required
    }
    fn unresolved_mode(&self) -> EnvelopeMode {
        self.0
    }
}

fn subscription_router(unresolved: EnvelopeMode, hits: &Hits) -> Router {
    let counted = hits.clone();
    let layer = EnvelopeLayer::builder(server_envelope(), AUDIENCE, SCHEMA)
        .policy(RequiredOps(unresolved))
        .rpc("")
        .build()
        .expect("layer");
    Router::new()
        .route(
            "/rpc/subscribe/{op_id}",
            get(move || {
                counted.hit();
                async { (StatusCode::OK, "subscribed") }
            }),
        )
        .layer(layer)
}

#[tokio::test]
async fn head_on_a_subscription_is_the_op_under_a_loose_unresolved_mode() {
    for unresolved in [EnvelopeMode::Optional, EnvelopeMode::Off] {
        let hits = Hits::default();
        let router = subscription_router(unresolved, &hits);
        let path = "/rpc/subscribe/model.Widget.subscribe";
        for method in [Method::GET, Method::HEAD] {
            let answer = send(&router, plain_request(method.clone(), path, b"")).await;
            assert_eq!(
                answer.status,
                StatusCode::UNAUTHORIZED,
                "{method} under unresolved {unresolved:?}"
            );
        }
        assert_eq!(hits.get(), 0, "{unresolved:?}: the handler must not run");
    }
}
