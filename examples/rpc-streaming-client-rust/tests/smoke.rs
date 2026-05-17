//! End-to-end smoke test for the generated typed streaming method.
//!
//! Spawns a tiny axum server that emits an `application/cbor-seq`
//! response chunk-per-item for `POST /rpc/procedure.ticks`, then
//! consumes it via the macro-generated `client.procedures().ticks(args)`
//! method. Verifies:
//!
//! 1. Items arrive in order.
//! 2. The decoder cleanly closes after the last item (no truncated
//!    final frame).
//! 3. The auth header configured on the `RequestAuthorizer` flows
//!    through to the server.
//!
//! Self-contained — does NOT depend on the `rpc-streaming-example`
//! server crate, so CI runs without orchestrating a second binary.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::post;
use bytes::Bytes;
use cratestack_client_rust::{ClientConfig, CratestackClient};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use rpc_streaming_client_rust_example::{
    StaticAuthId,
    cratestack_schema::{self, Tick, TickerArgs, procedures::ticks},
};
use url::Url;

const CBOR_SEQ: &str = "application/cbor-seq";

#[tokio::test]
async fn streams_each_tick_as_it_arrives() {
    let (base_url, _server) = spawn_server().await;

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
    let (base_url, _server) = spawn_server().await;

    // Build a client with NO authorizer — the mock server's handler
    // requires `x-auth-id` and returns 401 if it's missing. The error
    // path: the generated method returns Err(...) immediately; no
    // channel is opened.
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
// Mock server — chunked cbor-seq emitter for /rpc/procedure.ticks
// -----------------------------------------------------------------------------

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new().route("/rpc/procedure.ticks", post(handle_ticks));
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

async fn handle_ticks(headers: HeaderMap, body: Bytes) -> Response<Body> {
    // Auth: server example reads `x-auth-id` as a positive int. Mirror
    // that here so the missing-auth test exercises the same shape.
    let auth_id = headers
        .get("x-auth-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.parse::<i64>().ok());
    if auth_id.is_none() {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::from("missing or invalid x-auth-id"))
            .expect("response builds");
    }

    // The generated typed method sends the `<proc>::Args` envelope.
    let input: ticks::Args = CborCodec.decode(&body).expect("decode ticks::Args");
    let count = input.args.count.max(0);

    let pre_encoded: Vec<Vec<u8>> = (0..count)
        .map(|index| {
            CborCodec
                .encode(&Tick {
                    index,
                    value: input.args.start + index,
                })
                .expect("encode tick")
        })
        .collect();

    // Emit one cbor-seq frame per chunk with a tiny inter-frame delay
    // so the test exercises chunk-boundary handling, not just a single
    // fused read.
    let stream = async_stream::stream! {
        for bytes in pre_encoded {
            yield Ok::<_, Infallible>(Bytes::from(bytes));
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    };
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(CBOR_SEQ))
        .body(body)
        .expect("response builds")
}
