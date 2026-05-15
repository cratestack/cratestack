//! Bridge tests for `FlutterRuntime::execute_streamed`.
//!
//! Spawns a real axum server that emits `application/cbor-seq`
//! responses one chunk at a time, then drives the streaming FFI
//! callback API the way flutter_rust_bridge's `StreamSink<T>` would
//! drive it from Dart — except instead of crossing the FFI boundary,
//! the callback pushes into a `std::sync::mpsc::Sender` so the test
//! can inspect the per-chunk timing.

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::post;
use bytes::Bytes;
use cratestack_client_flutter::{
    FlutterChunkWire, FlutterRequest, FlutterRuntime, FlutterRuntimeCodec, FlutterRuntimeConfig,
    FlutterRuntimeEnvelope, FlutterRuntimeTransportConfig, FlutterStateStoreConfig,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamArgs {
    count: i64,
    /// Per-item delay in milliseconds (0 means no delay). Plain `u64`
    /// — not `Option<u64>` — to sidestep the known
    /// `serde_json::Value::Null → CBOR empty-array` bridge corruption
    /// in `cratestack-codec-cbor`.
    #[serde(default)]
    delay_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Tick {
    index: i64,
    label: String,
}

#[derive(Clone)]
struct AppState {
    codec: CborCodec,
}

const CBOR_SEQ: &str = "application/cbor-seq";

#[test]
fn execute_streamed_delivers_each_chunk_to_callback_then_end() {
    // Spin the server on a tokio runtime; the FFI side is sync.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    let body = serde_json::to_vec(&StreamArgs {
        count: 4,
        delay_ms: 0,
    })
    .expect("json body should encode");

    let collected = Arc::new(Mutex::new(Vec::<FlutterChunkWire>::new()));
    let sink = Arc::clone(&collected);
    runtime
        .execute_streamed(
            FlutterRequest {
                method: "POST".to_owned(),
                path: "/stream".to_owned(),
                canonical_query: None,
                headers: Vec::new(),
                body,
            },
            move |chunk| {
                sink.lock().unwrap().push(chunk);
                true
            },
        )
        .expect("execute_streamed should complete");

    let chunks = Arc::try_unwrap(collected).unwrap().into_inner().unwrap();
    // Four items + one End marker.
    assert_eq!(chunks.len(), 5, "expected 4 items + End: {chunks:?}");
    for (i, chunk) in chunks.iter().enumerate().take(4) {
        match chunk {
            FlutterChunkWire::Item(bytes) => {
                let tick: Tick = CborCodec.decode(bytes).expect("decode item");
                assert_eq!(tick.index, i as i64);
                assert_eq!(tick.label, format!("tick-{i}"));
            }
            other => panic!("expected Item at {i}, got {other:?}"),
        }
    }
    assert!(
        matches!(chunks[4], FlutterChunkWire::End),
        "final chunk should be End, got {:?}",
        chunks[4],
    );
}

#[test]
fn execute_streamed_respects_callback_cancellation() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    let body = serde_json::to_vec(&StreamArgs {
        count: 50,
        delay_ms: 5,
    })
    .expect("json body should encode");

    let counter = Arc::new(Mutex::new(0usize));
    let counter_ref = Arc::clone(&counter);
    let result = runtime.execute_streamed(
        FlutterRequest {
            method: "POST".to_owned(),
            path: "/stream".to_owned(),
            canonical_query: None,
            headers: Vec::new(),
            body,
        },
        move |chunk| {
            if matches!(chunk, FlutterChunkWire::Item(_)) {
                let mut n = counter_ref.lock().unwrap();
                *n += 1;
                // Cancel after the 3rd item.
                return *n < 3;
            }
            true
        },
    );

    assert!(result.is_ok(), "callback-cancel is not an error");
    assert_eq!(
        *counter.lock().unwrap(),
        3,
        "callback should have seen exactly 3 items before cancelling",
    );
}

#[test]
fn execute_streamed_surfaces_server_error_as_flutter_runtime_error() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    // Empty body — `/stream-error` returns 500 unconditionally.
    let body = serde_json::to_vec(&StreamArgs {
        count: 0,
        delay_ms: 0,
    })
    .expect("json body should encode");

    let result = runtime.execute_streamed(
        FlutterRequest {
            method: "POST".to_owned(),
            path: "/stream-error".to_owned(),
            canonical_query: None,
            headers: Vec::new(),
            body,
        },
        |_chunk| true,
    );

    let error = result.expect_err("server returned 500 — call should fail before stream begins");
    assert_eq!(
        error.http_status,
        Some(500),
        "error should carry the upstream 500: {error:?}",
    );
    assert!(
        error.message.contains("synthetic upstream failure")
            || error.message.contains("INTERNAL_ERROR")
            || error.message.contains("500"),
        "error message should reflect the upstream failure: {}",
        error.message,
    );
}

// -----------------------------------------------------------------------------
// `rpc_call_streamed` — RPC streaming bridge (POST /rpc/{op_id})
//
// Mirrors the three REST tests above but exercises the dedicated
// `rpc_call_streamed(op_id, input, headers, on_chunk)` entrypoint, so
// the FFI path that consuming Flutter apps actually use for
// `transport rpc` schemas is covered end-to-end.
// -----------------------------------------------------------------------------

#[test]
fn rpc_call_streamed_delivers_each_chunk_to_callback_then_end() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    let input = serde_json::to_vec(&StreamArgs {
        count: 4,
        delay_ms: 0,
    })
    .expect("json body should encode");

    let collected = Arc::new(Mutex::new(Vec::<FlutterChunkWire>::new()));
    let sink = Arc::clone(&collected);
    runtime
        .rpc_call_streamed("test.stream", input, Vec::new(), move |chunk| {
            sink.lock().unwrap().push(chunk);
            true
        })
        .expect("rpc_call_streamed should complete");

    let chunks = Arc::try_unwrap(collected).unwrap().into_inner().unwrap();
    assert_eq!(chunks.len(), 5, "expected 4 items + End: {chunks:?}");
    for (i, chunk) in chunks.iter().enumerate().take(4) {
        match chunk {
            FlutterChunkWire::Item(bytes) => {
                let tick: Tick = CborCodec.decode(bytes).expect("decode item");
                assert_eq!(tick.index, i as i64);
                assert_eq!(tick.label, format!("tick-{i}"));
            }
            other => panic!("expected Item at {i}, got {other:?}"),
        }
    }
    assert!(
        matches!(chunks[4], FlutterChunkWire::End),
        "final chunk should be End, got {:?}",
        chunks[4],
    );
}

#[test]
fn rpc_call_streamed_respects_callback_cancellation() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    let input = serde_json::to_vec(&StreamArgs {
        count: 50,
        delay_ms: 5,
    })
    .expect("json body should encode");

    let counter = Arc::new(Mutex::new(0usize));
    let counter_ref = Arc::clone(&counter);
    let result =
        runtime.rpc_call_streamed("test.stream", input, Vec::new(), move |chunk| {
            if matches!(chunk, FlutterChunkWire::Item(_)) {
                let mut n = counter_ref.lock().unwrap();
                *n += 1;
                return *n < 3;
            }
            true
        });

    assert!(result.is_ok(), "callback-cancel is not an error");
    assert_eq!(
        *counter.lock().unwrap(),
        3,
        "callback should have seen exactly 3 items before cancelling",
    );
}

#[test]
fn rpc_call_streamed_surfaces_server_error_as_flutter_runtime_error() {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build");
    let (base_url, _server) = rt.block_on(spawn_server());

    let runtime = FlutterRuntime::new(FlutterRuntimeConfig {
        base_url: base_url.to_string(),
        state_store: FlutterStateStoreConfig::InMemory,
        transport: FlutterRuntimeTransportConfig {
            codec: FlutterRuntimeCodec::Cbor,
            envelope: FlutterRuntimeEnvelope::None,
        },
    })
    .expect("flutter runtime should build");

    let input = serde_json::to_vec(&StreamArgs {
        count: 0,
        delay_ms: 0,
    })
    .expect("json body should encode");

    let result = runtime.rpc_call_streamed("test.error", input, Vec::new(), |_chunk| true);

    let error = result.expect_err("server returned 500 — call should fail before stream begins");
    assert_eq!(
        error.http_status,
        Some(500),
        "error should carry the upstream 500: {error:?}",
    );
    assert!(
        error.message.contains("synthetic upstream failure")
            || error.message.contains("INTERNAL_ERROR")
            || error.message.contains("500"),
        "error message should reflect the upstream failure: {}",
        error.message,
    );
}

// -----------------------------------------------------------------------------
// Server helpers (chunked cbor-seq emitter + a 5xx route)
// -----------------------------------------------------------------------------

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/stream", post(handle_stream))
        .route("/stream-error", post(handle_error))
        // Same handlers, exposed under `/rpc/{op_id}` paths so the
        // `rpc_call_streamed` tests below can hit them.
        .route("/rpc/test.stream", post(handle_stream))
        .route("/rpc/test.error", post(handle_error))
        .with_state(AppState { codec: CborCodec });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have an address");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    let base_url = Url::parse(&format!("http://{address}/")).expect("base URL should parse");
    (base_url, handle)
}

async fn handle_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response<Body> {
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !accept.contains(CBOR_SEQ) {
        return Response::builder()
            .status(StatusCode::NOT_ACCEPTABLE)
            .body(Body::from(format!("client did not accept cbor-seq: {accept}")))
            .expect("response should build");
    }

    let args: StreamArgs = state.codec.decode(&body).expect("decode StreamArgs");
    let count = args.count.max(0) as usize;
    let delay = if args.delay_ms > 0 {
        Some(Duration::from_millis(args.delay_ms))
    } else {
        None
    };

    let pre_encoded: Vec<Vec<u8>> = (0..count)
        .map(|index| {
            state
                .codec
                .encode(&Tick {
                    index: index as i64,
                    label: format!("tick-{index}"),
                })
                .expect("encode tick")
        })
        .collect();

    let stream = async_stream::stream! {
        for bytes in pre_encoded {
            yield Ok::<_, Infallible>(Bytes::from(bytes));
            if let Some(d) = delay {
                tokio::time::sleep(d).await;
            }
        }
    };
    let body = Body::from_stream(stream);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(CBOR_SEQ))
        .body(body)
        .expect("response should build")
}

async fn handle_error(_state: State<AppState>) -> Response<Body> {
    let codec = CborCodec;
    let body = codec
        .encode(&cratestack_core::CoolErrorResponse {
            code: "INTERNAL_ERROR".to_owned(),
            message: "synthetic upstream failure".to_owned(),
            details: None,
        })
        .expect("encode error");
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, HeaderValue::from_static("application/cbor"))
        .body(Body::from(body))
        .expect("response should build")
}
