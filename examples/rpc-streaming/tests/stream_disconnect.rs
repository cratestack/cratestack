//! cratestack#283's top flagged risk: a client disconnecting mid-stream
//! must actually stop server-side item production shortly afterward,
//! not leak a task that runs the stream to completion regardless of
//! whether anyone is still listening.
//!
//! This drives the real generated router (`build_router_with`) over a
//! genuine TCP connection (not `tower::ServiceExt::oneshot`, which
//! doesn't model a real socket close) and observes `Procedures::produced`
//! — an atomic counter incremented once per item *actually produced*
//! server-side, independent of whether it ever reaches a client —
//! before and after abruptly closing the client's `TcpStream`.

use std::sync::atomic::Ordering;
use std::time::Duration;

use cratestack::CoolCodec;
use cratestack_codec_cbor::CborCodec;
use rpc_streaming_example::{Procedures, build_router_with, schema};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Deliberately huge relative to how long this test actually waits — if
/// cancellation is broken (production silently runs to completion in
/// the background), a naive "did it reach COUNT" assertion would need
/// to wait for the full `COUNT * TICK_INTERVAL` (20ms — see
/// `rpc_streaming_example::TICK_INTERVAL`) to observe the failure. Using
/// a huge count instead means a broken implementation is caught almost
/// immediately: production would still be climbing steadily every time
/// we sample it, long after a correct implementation has stopped dead.
const COUNT: i64 = 100_000;

#[tokio::test]
async fn client_disconnect_stops_server_side_item_production() {
    let procedures = Procedures::default();
    let produced = procedures.produced.clone();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = build_router_with(procedures);
    tokio::spawn(async move {
        cratestack::axum::serve(listener, app).await.unwrap();
    });

    let body = CborCodec
        .encode(&schema::procedures::ticks::Args {
            args: schema::TickerArgs {
                start: 0,
                count: COUNT,
            },
        })
        .unwrap();

    let mut stream = TcpStream::connect(addr).await.unwrap();
    let request = format!(
        "POST /rpc/procedure.ticks HTTP/1.1\r\n\
         Host: {addr}\r\n\
         Content-Type: {content_type}\r\n\
         Accept: {accept}\r\n\
         x-auth-id: 1\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\
         \r\n",
        content_type = CborCodec::CONTENT_TYPE,
        accept = cratestack::CBOR_SEQUENCE_CONTENT_TYPE,
        len = body.len(),
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.write_all(&body).await.unwrap();

    // Read until the server has clearly started streaming (past the
    // HTTP headers and into body bytes) — proving this isn't "the
    // connection never got far enough to start producing anything."
    let mut buf = vec![0u8; 4096];
    let mut total_read = 0usize;
    loop {
        let n = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf[..]))
            .await
            .expect("server should send response headers + some body promptly")
            .unwrap();
        assert!(n > 0, "connection closed before any data arrived");
        total_read += n;
        // Comfortably past a `200 OK` + headers block; guarantees we're
        // into the streamed body, i.e. production has genuinely begun.
        if total_read > 200 {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    let produced_before_disconnect = produced.load(Ordering::SeqCst);
    assert!(
        produced_before_disconnect > 0,
        "expected at least one item produced before disconnecting"
    );

    // Simulate the client going away: close both halves of the TCP
    // connection immediately, mid-stream, with items still pending.
    drop(stream);

    // Grace period for the server to notice the broken pipe on its next
    // write attempt and drop the response body (and, with it, the
    // `Stream` — see `crates/cratestack-axum/src/transport/
    // stream_sequence.rs`'s `encode_items_stream`, which owns the only
    // handle to the underlying `async_stream`-generated producer).
    // `TICK_INTERVAL` is 20ms, so 500ms is a wide multiple of the
    // production cadence — plenty of margin without making the test
    // slow.
    tokio::time::sleep(Duration::from_millis(500)).await;
    let produced_after_grace_period = produced.load(Ordering::SeqCst);

    // If cancellation is broken, production keeps running at its normal
    // cadence (one item / `TICK_INTERVAL` = 20ms) regardless of the
    // disconnect — 500ms of grace period would net roughly 25 more
    // items. A correct implementation stops within about one tick of
    // hyper noticing the broken pipe on its next write attempt, so the
    // delta should be tiny. Threshold picked well below that ~25 to
    // give a real gap between "stopped" and "still running", not just
    // barely under the uncancelled number.
    let produced_after_disconnect = produced_after_grace_period - produced_before_disconnect;
    assert!(
        produced_after_disconnect < 5,
        "server kept producing items ({produced_after_disconnect} more) long after the \
         client disconnected — the stream task is not being cancelled on client \
         disconnect (before: {produced_before_disconnect}, after grace period: \
         {produced_after_grace_period}, target if uncancelled would approach {COUNT})",
    );
}
