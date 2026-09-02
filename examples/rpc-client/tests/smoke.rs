//! End-to-end smoke test for the generated typed RPC client.
//!
//! Spawns the REAL `rpc-procedures-example` server
//! (`rpc_procedures_example::build_router()` — the exact router the
//! server binary serves) in-process, then drives it through the
//! macro-generated `client::Client`. This exercises the actual HTTP +
//! CBOR content-negotiation path end to end rather than stubbing it.
//!
//! Verifies:
//!
//! 1. A **unary** call (`client.procedures().greet(&args).await`) round-trips
//!    a single `POST /rpc/procedure.greet`.
//! 2. A **batch** call queues two `BatchableCall`s into one
//!    `BatchBuilder`, sends one `POST /rpc/batch`, and each result is
//!    collected by its `BatchHandle` — with the server's in-memory
//!    counter confirming both increments landed.
//! 3. **Auth flows through**: the `x-auth-id` header from the
//!    `RequestAuthorizer` reaches the server, and a client with no
//!    authorizer is denied (surfaces as a remote error).
//!
//! Depends on `rpc-procedures-example` as a dev-dependency (both are
//! workspace members; the router is built and served in-process on an
//! ephemeral port — no orchestration of the server binary needed).

use std::sync::Arc;

use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use rpc_client_example::{
    StaticAuthId,
    cratestack_schema::{self, CounterDelta, GreetArgs, procedures},
};
use url::Url;

#[tokio::test]
async fn unary_greet_round_trips() {
    let (base_url, _server) = spawn_real_server().await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_request_authorizer(Arc::new(StaticAuthId(1)));
    let client = cratestack_schema::client::Client::new(runtime);

    let args = procedures::greet::Args {
        args: GreetArgs {
            name: "world".to_owned(),
        },
    };
    let reply = client
        .procedures()
        .greet(&args)
        .await
        .expect("unary call should succeed");
    assert_eq!(reply.message, "hello, world!");
}

#[tokio::test]
async fn batch_round_trip_runs_both_ops_in_one_call() {
    let (base_url, _server) = spawn_real_server().await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_request_authorizer(Arc::new(StaticAuthId(1)));
    let client = cratestack_schema::client::Client::new(runtime);

    let mut batch = client.batch();

    let handle_5 = client
        .procedures()
        .increment(&procedures::increment::Args {
            args: CounterDelta { by: 5 },
        })
        .queue(&mut batch);
    let handle_3 = client
        .procedures()
        .increment(&procedures::increment::Args {
            args: CounterDelta { by: 3 },
        })
        .queue(&mut batch);

    let mut results = batch.send().await.expect("batch should send");

    let total_5 = results.take(handle_5).expect("first frame");
    let total_3 = results.take(handle_3).expect("second frame");
    assert_eq!(total_5.total, 5, "first increment lands at 5");
    // The order of results is the order ops were queued: 5 then 3.
    assert_eq!(total_3.total, 8, "second increment lands at 8");
}

#[tokio::test]
async fn missing_auth_surfaces_as_remote_error() {
    let (base_url, _server) = spawn_real_server().await;

    // No authorizer -> no `x-auth-id` header -> the server's
    // `@allow(auth() != null)` denies the call.
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let args = procedures::greet::Args {
        args: GreetArgs {
            name: "stranger".to_owned(),
        },
    };
    let result = client.procedures().greet(&args).await;
    assert!(
        result.is_err(),
        "missing auth should surface as a remote error",
    );
}

// ---------------------------------------------------------------------------
// Real server — the exact router the `rpc-procedures-example` binary serves
// ---------------------------------------------------------------------------

async fn spawn_real_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = rpc_procedures_example::build_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener.local_addr().expect("listener address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL parses");
    (base_url, handle)
}
