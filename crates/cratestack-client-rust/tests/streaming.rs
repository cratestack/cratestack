//! End-to-end tests for the streaming client API.
//!
//! Spawns a real axum server that emits `application/cbor-seq`
//! responses one chunk at a time (with optional delays between
//! chunks to assert that items arrive incrementally rather than as
//! one buffered blob), then exercises:
//!
//! - `CratestackClient::post_list_streamed` — typed Rust caller path.
//! - `RuntimeHandle::execute_streamed` — FFI callback path.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Response, StatusCode, header};
use axum::routing::post;
use bytes::Bytes;
use cratestack_client_rust::{
    ClientConfig, CratestackClient, RuntimeChunkWire, RuntimeCodecConfig, RuntimeConfigWire,
    RuntimeEnvelopeConfig, RuntimeHandle, RuntimeRequestWire,
    RuntimeStateStoreConfig, RuntimeTransportConfig,
};
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StreamArgs {
    count: i64,
    /// Per-item delay in milliseconds. Zero means no delay. (We use a
    /// plain `u64` rather than `Option<u64>` so the test can run the
    /// FFI bridge path without tripping the known
    /// `serde_json::Value::Null → CBOR empty-array` round-trip
    /// corruption in `cratestack-codec-cbor`.)
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

async fn handle_stream(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response<Body> {
    // Accept must include cbor-seq; otherwise the test setup is wrong.
    let accept = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !accept.contains(CBOR_SEQ) {
        return Response::builder()
            .status(StatusCode::NOT_ACCEPTABLE)
            .body(Body::from(format!(
                "client did not accept cbor-seq: {accept}"
            )))
            .expect("response should build");
    }

    let args: StreamArgs = state.codec.decode(&body).expect("decode StreamArgs");
    let count = args.count.max(0) as usize;
    let delay = if args.delay_ms > 0 {
        Some(Duration::from_millis(args.delay_ms))
    } else {
        None
    };

    // Pre-encode each item so the streaming logic stays simple.
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

    // Wrap pre-encoded bytes in a chunked Body. Each item becomes one
    // chunk, with `delay` (if set) between chunks. Real cratestack
    // servers do the same via `encode_transport_sequence_result_for`.
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
    // Emit a CBOR-encoded CoolErrorResponse with a 5xx status.
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
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/cbor"),
        )
        .body(Body::from(body))
        .expect("response should build")
}

async fn spawn_server() -> (Url, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .route("/stream", post(handle_stream))
        .route("/stream-error", post(handle_error))
        .with_state(AppState { codec: CborCodec });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener bind");
    let address = listener.local_addr().expect("addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server run");
    });
    let url = Url::parse(&format!("http://{address}/")).expect("url parse");
    (url, handle)
}

// -----------------------------------------------------------------------------
// Typed API: CratestackClient::post_list_streamed
// -----------------------------------------------------------------------------

#[tokio::test]
async fn post_list_streamed_yields_items_in_order() {
    let (base_url, server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let mut rx: mpsc::Receiver<_> = client
        .post_list_streamed::<_, Tick>(
            "/stream",
            &StreamArgs {
                count: 4,
                delay_ms: 0,
            },
            &[],
        )
        .await
        .expect("post_list_streamed should start");

    let mut received = Vec::new();
    while let Some(item) = rx.recv().await {
        received.push(item.expect("each item should decode"));
    }
    server.abort();

    assert_eq!(received.len(), 4);
    for (i, tick) in received.iter().enumerate() {
        assert_eq!(tick.index, i as i64);
        assert_eq!(tick.label, format!("tick-{i}"));
    }
}

#[tokio::test]
async fn post_list_streamed_delivers_items_before_stream_close() {
    // Proves the streaming path is genuinely streaming: with a 50ms
    // delay between server chunks, the first item should arrive long
    // before the last. A buffered consumer would block until all 8
    // items + 7*50ms had elapsed.
    let (base_url, server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let started = Instant::now();
    let mut rx = client
        .post_list_streamed::<_, Tick>(
            "/stream",
            &StreamArgs {
                count: 8,
                delay_ms: 50,
            },
            &[],
        )
        .await
        .expect("post_list_streamed should start");

    let first = rx.recv().await.expect("first item").expect("decode");
    let first_at = started.elapsed();
    assert_eq!(first.index, 0);

    // Drain the rest.
    let mut count = 1usize;
    while let Some(item) = rx.recv().await {
        item.expect("decode subsequent item");
        count += 1;
    }
    let total_at = started.elapsed();
    server.abort();

    assert_eq!(count, 8);
    // First item should arrive well before the total — proving items
    // arrive incrementally, not as one buffered blob at the end.
    let total_expected = Duration::from_millis(7 * 50);
    assert!(
        first_at < total_at / 2,
        "first item arrived at {first_at:?}, total {total_at:?} — streaming should yield first item early (expected delay between first and last ~{total_expected:?})",
    );
}

#[tokio::test]
async fn post_list_streamed_surfaces_server_error_as_remote() {
    let (base_url, server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let result = client
        .post_list_streamed::<_, Tick>(
            "/stream-error",
            &StreamArgs {
                count: 0,
                delay_ms: 0,
            },
            &[],
        )
        .await;
    server.abort();

    let error = result.expect_err("server returned 500 — call should fail before stream begins");
    let message = format!("{error:?}");
    assert!(
        message.contains("synthetic upstream failure")
            || message.contains("INTERNAL_ERROR")
            || message.contains("500"),
        "error message should reflect the upstream failure: {message}",
    );
}

#[tokio::test]
async fn post_list_streamed_dropping_receiver_cancels_the_stream() {
    // Dropping the receiver mid-stream should release server resources —
    // the spawned pump task observes the sender failing and stops.
    // This test asserts the receiver-drop path doesn't panic and the
    // server's chunk emission stops sometime after.
    let (base_url, server) = spawn_server().await;
    let client = CratestackClient::new(ClientConfig::new(base_url), CborCodec);

    let mut rx = client
        .post_list_streamed::<_, Tick>(
            "/stream",
            &StreamArgs {
                count: 100,
                delay_ms: 10,
            },
            &[],
        )
        .await
        .expect("streaming starts");

    // Take three items, then drop the receiver.
    for _ in 0..3 {
        rx.recv().await.expect("item").expect("decode");
    }
    drop(rx);
    // Server task may continue producing chunks for a bit until the
    // underlying TCP write fails — we don't assert the exact stop time,
    // only that nothing panics here.
    tokio::time::sleep(Duration::from_millis(100)).await;
    server.abort();
}

// -----------------------------------------------------------------------------
// FFI callback API: RuntimeHandle::execute_streamed
// -----------------------------------------------------------------------------

fn runtime(base_url: &str) -> RuntimeHandle {
    RuntimeHandle::new(RuntimeConfigWire {
        base_url: base_url.to_owned(),
        state_store: RuntimeStateStoreConfig::InMemory,
        transport: RuntimeTransportConfig {
            codec: RuntimeCodecConfig::Cbor,
            envelope: RuntimeEnvelopeConfig::None,
        },
    })
    .expect("runtime should build")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn execute_streamed_delivers_each_item_to_the_callback() {
    let (base_url, server) = spawn_server().await;
    // RuntimeHandle owns its own current-thread runtime; we drive it
    // off the test runtime's threadpool via spawn_blocking so the
    // block_on inside doesn't deadlock.
    let url_str = base_url.as_str().to_owned();
    let handle = tokio::task::spawn_blocking(move || {
        let runtime = runtime(&url_str);
        let request = RuntimeRequestWire {
            method: "POST".into(),
            path: "/stream".into(),
            canonical_query: None,
            headers: Vec::new(),
            // The FFI bridge normalizes request bodies to JSON; the
            // RuntimeHandle re-encodes to the configured codec
            // (CBOR/JSON) before sending on the wire.
            body: serde_json::to_vec(&StreamArgs {
                count: 3,
                delay_ms: 0,
            })
            .expect("encode args"),
        };

        let collected = Arc::new(std::sync::Mutex::new(Vec::<RuntimeChunkWire>::new()));
        let sink = Arc::clone(&collected);
        runtime
            .execute_streamed(request, move |chunk| {
                sink.lock().unwrap().push(chunk);
                true
            })
            .expect("execute_streamed completes");
        Arc::try_unwrap(collected).unwrap().into_inner().unwrap()
    });

    let chunks = handle.await.expect("blocking task");
    server.abort();

    // Three items + one End marker.
    assert_eq!(chunks.len(), 4, "expected 3 items + End, got {chunks:?}");
    for (i, chunk) in chunks.iter().enumerate().take(3) {
        match chunk {
            RuntimeChunkWire::Item(bytes) => {
                let tick: Tick = CborCodec.decode(bytes).expect("decode item");
                assert_eq!(tick.index, i as i64);
            }
            other => panic!("expected Item at index {i}, got {other:?}"),
        }
    }
    assert!(
        matches!(chunks[3], RuntimeChunkWire::End),
        "last chunk must be End, got {:?}",
        chunks[3],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn execute_streamed_respects_callback_cancellation() {
    let (base_url, server) = spawn_server().await;
    let url_str = base_url.as_str().to_owned();
    let handle = tokio::task::spawn_blocking(move || {
        let runtime = runtime(&url_str);
        let request = RuntimeRequestWire {
            method: "POST".into(),
            path: "/stream".into(),
            canonical_query: None,
            headers: Vec::new(),
            body: serde_json::to_vec(&StreamArgs {
                count: 100,
                delay_ms: 5,
            })
            .expect("encode args"),
        };

        let seen = Arc::new(std::sync::Mutex::new(0usize));
        let counter = Arc::clone(&seen);
        let result = runtime.execute_streamed(request, move |chunk| {
            if matches!(chunk, RuntimeChunkWire::Item(_)) {
                let mut n = counter.lock().unwrap();
                *n += 1;
                // Cancel after the 5th item.
                return *n < 5;
            }
            true
        });
        let final_count = *seen.lock().unwrap();
        (result, final_count)
    });

    let (result, final_count) = handle.await.expect("blocking task");
    server.abort();

    assert!(result.is_ok(), "cancellation is not an error");
    assert_eq!(
        final_count, 5,
        "callback should have seen exactly 5 items before cancelling"
    );
}
