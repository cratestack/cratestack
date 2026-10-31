//! cratestack#1261 on the RPC binding, unary calls (`/rpc/batch` refuses an
//! `Idempotency-Key` header outright): the scenarios of
//! `authn_idempotency_rest.rs`, through the generated `rpc_router()`.

mod authn_support;
#[macro_use]
mod authn_idempotency_support;

use authn_idempotency_support::{Provider, Runs, Transport, keyed};
use cratestack::include_server_schema;
use cratestack_codec_json::JsonCodec;

include_server_schema!("tests/fixtures/authn_idempotency_rpc.cstack", db = None);

registry!();

fn transport(runs: &Runs) -> Transport {
    Transport {
        app: keyed(cratestack_schema::axum::rpc_router(
            cratestack_schema::Cratestack::builder().build(),
            Procedures(runs.clone()),
            (),
            JsonCodec,
            Provider,
            cratestack::DEFAULT_BODY_LIMIT_BYTES,
        )),
        path: |procedure| format!("/rpc/procedure.{procedure}"),
    }
}

#[tokio::test]
async fn a_step_up_refusal_is_not_recorded() {
    let runs = Runs::default();
    authn_idempotency_support::a_step_up_refusal_is_not_recorded(transport(&runs), &runs).await;
}

#[tokio::test]
async fn an_authenticate_refusal_is_not_recorded() {
    let runs = Runs::default();
    authn_idempotency_support::an_authenticate_refusal_is_not_recorded(transport(&runs), &runs)
        .await;
}

#[tokio::test]
async fn a_policy_refusal_is_not_recorded() {
    let runs = Runs::default();
    authn_idempotency_support::a_policy_refusal_is_not_recorded(transport(&runs), &runs).await;
}

#[tokio::test]
async fn a_refusal_from_the_body_is_recorded() {
    let runs = Runs::default();
    authn_idempotency_support::a_refusal_from_the_body_is_recorded(transport(&runs), &runs).await;
}
