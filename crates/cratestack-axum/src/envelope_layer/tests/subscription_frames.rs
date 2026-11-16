//! `subscriptions(..)` (cratestack#1378, part A) is inert: the layer stores
//! the generated `SUBSCRIPTION_FRAMES` table, defaults to empty, and no
//! request is decided by it yet. A signed subscription open is still the
//! sealed `406` before its handler runs, marked or not.

use axum::Router;
use axum::routing::get;
use cratestack_core::{FramesMode, request_digest};
use http::{Method, StatusCode};

use super::fixtures::Hits;
use super::support::*;
use crate::envelope_layer::{EnvelopeLayer, EnvelopeLayerBuilder, EnvelopeMode};

const TABLE: &[(&str, FramesMode)] = &[("model.Widget.subscribe", FramesMode::Unsigned)];

fn builder() -> EnvelopeLayerBuilder {
    EnvelopeLayer::builder(server_envelope(), AUDIENCE, CONTRACTS)
        .policy(EnvelopeMode::Required)
        .rpc("")
}

#[test]
fn the_table_defaults_to_empty_and_is_stored_when_given() {
    let none = builder().build().expect("layer");
    assert!(
        none.config.subscriptions.is_empty(),
        "fail closed by default"
    );
    let some = builder().subscriptions(TABLE).build().expect("layer");
    assert_eq!(some.config.subscriptions, TABLE);
}

#[tokio::test]
async fn a_marked_subscription_is_still_the_sealed_406_until_the_open_is_signed() {
    for layer in [
        builder().build().expect("layer"),
        builder().subscriptions(TABLE).build().expect("layer"),
    ] {
        let hits = Hits::default();
        let counted = hits.clone();
        let handler = move || {
            counted.hit();
            async { StatusCode::NO_CONTENT }
        };
        let router = Router::new()
            .route("/rpc/subscribe/{op_id}", get(handler))
            .layer(layer);
        let call = Call::new(Method::GET, "subscribe/model.Widget.subscribe", &[]);
        let sealed = call.seal(&[]).await;
        let path = "/rpc/subscribe/model.Widget.subscribe";
        let answer = send(&router, cose_request(Method::GET, path, sealed.clone())).await;
        assert_eq!(answer.status, StatusCode::NOT_ACCEPTABLE);
        assert_eq!(hits.get(), 0, "the handler must not run");
        call.open(request_digest(&sealed), answer.status, answer.body)
            .await
            .expect("sealed");
    }
}
