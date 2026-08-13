//! End-to-end smoke test for the generated typed streaming method.
//!
//! Spawns the REAL `rpc-streaming-example` server (`rpc_streaming_example::
//! build_router()` — the exact router `cargo run -p rpc-streaming-example`
//! serves) in-process, then consumes it via the macro-generated
//! `client.procedures().ticks(args)` method. This exercises the actual
//! HTTP content-negotiation path end to end: the generated client sends
//! `Accept: application/cbor-seq, application/cbor[, application/json]`
//! (preferring cbor-seq) and the server picks a response `Content-Type`
//! via `select_response_content_type` in `cratestack-axum`.
//!
//! This test previously ran against a hand-rolled mock server that always
//! answered `application/cbor-seq` regardless of the request's `Accept`
//! header — which meant it passed even when server-side negotiation
//! ignored the client's preference order entirely and always returned
//! buffered `application/cbor` instead (the bug fixed alongside this
//! test: `select_response_content_type` used to walk its own
//! `response_types` list, cbor-first, rather than the client's `Accept`
//! order/`q=` weights). A mock that hardcodes the "right" answer can't
//! catch a negotiation regression; only the real server's negotiation
//! logic can. See `crates/cratestack-axum/src/transport/media_type.rs`
//! and its `mod tests` for focused unit coverage of the negotiation
//! function itself.
//!
//! Verifies:
//!
//! 1. Items arrive in order.
//! 2. The decoder cleanly closes after the last item (no truncated
//!    final frame).
//! 3. The auth header configured on the `RequestAuthorizer` flows
//!    through to the server.
//!
//! Depends on `rpc-streaming-example` as a dev-dependency (both are
//! workspace members; no orchestration of a second binary needed — the
//! router is built and served in-process on an ephemeral port).

use std::sync::Arc;

use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use rpc_streaming_client_rust_example::{
    StaticAuthId,
    cratestack_schema::{self, Tick, TickerArgs, procedures::ticks},
};
use url::Url;

#[tokio::test]
async fn streams_each_tick_as_it_arrives() {
    let (base_url, _server) = spawn_real_server().await;

    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec)
        .with_request_authorizer(Arc::new(StaticAuthId(1)));
    let client = cratestack_schema::client::Client::new(runtime);

    let args = ticks::Args {
        args: TickerArgs {
            start: 100,
            count: 5,
        },
    };

    let mut rx = client
        .procedures()
        .ticks(&args)
        .await
        .expect("typed streaming method should open the stream");

    let mut received = Vec::<Tick>::new();
    while let Some(item) = rx.recv().await {
        received.push(item.expect("per-item should not error"));
    }

    assert_eq!(received.len(), 5, "should receive all 5 ticks");
    for (i, tick) in received.iter().enumerate() {
        assert_eq!(tick.index, i as i64);
        assert_eq!(tick.value, 100 + i as i64);
    }
}

#[tokio::test]
async fn missing_auth_header_surfaces_as_remote_error_before_stream_opens() {
    let (base_url, _server) = spawn_real_server().await;

    // Build a client with NO authorizer — the real server's `ticks`
    // procedure is `@allow(auth() != null)` and the example's
    // `HeaderAuthProvider` authenticates only when `x-auth-id` is
    // present, so an anonymous request is denied. The error path: the
    // generated method returns Err(...) immediately; no channel is
    // opened.
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    let args = ticks::Args {
        args: TickerArgs { start: 0, count: 1 },
    };

    let result = client.procedures().ticks(&args).await;
    assert!(
        result.is_err(),
        "missing auth should surface as Err before the channel opens",
    );
}

// -----------------------------------------------------------------------------
// Real server — the exact router `rpc-streaming-example`'s binary serves
// -----------------------------------------------------------------------------

async fn spawn_real_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = rpc_streaming_example::build_router();
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
