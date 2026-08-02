//! Wire-level proof of cratestack#283's core claim: item N arrives over
//! the *actual HTTP response* before the server has produced item N+1
//! — against the real generated router (`build_router_with`), driven
//! over a real TCP connection via `reqwest`, not a hand-rolled fake
//! server.
//!
//! `tests/smoke.rs` (pre-#283) proves the wire *content* is correct but
//! `to_bytes()`s the whole response first, so it structurally cannot
//! distinguish buffered from incremental delivery — that's the gap this
//! file closes. `tests/stream_incremental.rs` (from cratestack#282)
//! proves incrementality at the `ProcedureRegistry` trait boundary, one
//! layer below HTTP; this file proves it survived all the way through
//! `cratestack-axum`'s response encoding onto the wire.
//!
//! Both client and server run in the same test process, sharing an
//! `Arc<Mutex<Vec<Instant>>>` (`Procedures::produced_at`) — that's what
//! makes a direct `client_arrival_instant < server_production_instant`
//! comparison possible without clock-sync concerns (both instants come
//! from the same `Instant` clock).

use std::time::{Duration, Instant};

use cratestack::CoolCodec;
use cratestack::futures::StreamExt;
use cratestack_client_rust::CborSeqChunkDecoder;
use cratestack_codec_cbor::CborCodec;
use rpc_streaming_example::{Procedures, build_router_with, schema};
use tokio::net::TcpListener;

const COUNT: i64 = 8;

#[tokio::test]
async fn item_n_arrives_over_the_wire_before_server_produces_item_n_plus_1() {
    let procedures = Procedures::default();
    let produced_at = procedures.produced_at.clone();

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

    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://{addr}/rpc/procedure.ticks"))
        .header("content-type", CborCodec::CONTENT_TYPE)
        .header("accept", cratestack::CBOR_SEQUENCE_CONTENT_TYPE)
        .header("x-auth-id", "1")
        .body(body)
        .send()
        .await
        .expect("request should succeed");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    // Record wall-clock arrival time of each complete cbor-seq item as
    // it's boundary-scanned out of the byte stream — using the same
    // `CborSeqChunkDecoder` the real Rust client uses, per this
    // ticket's own "verify with ... the existing Rust client's
    // boundary-scanner" guidance.
    let mut byte_stream = response.bytes_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    let mut client_arrivals: Vec<Instant> = Vec::new();
    while client_arrivals.len() < COUNT as usize {
        let chunk = byte_stream
            .next()
            .await
            .expect("stream should not end before all items arrive")
            .expect("chunk read should succeed");
        let items = decoder.feed_chunk(&chunk).expect("chunk should decode");
        for _ in items {
            client_arrivals.push(Instant::now());
        }
    }

    // Give the server a moment to record its own last production
    // timestamp (it's recorded a few microseconds before the `yield`
    // that puts the item on the wire, so it's always already there by
    // the time the corresponding chunk is read above — this is just
    // paranoia against scheduling jitter on the very last item).
    tokio::time::sleep(Duration::from_millis(20)).await;
    let server_produced_at = produced_at.lock().unwrap().clone();
    assert_eq!(
        server_produced_at.len(),
        COUNT as usize,
        "server should have produced exactly COUNT items"
    );

    // The actual claim: for every item except the last, the client
    // observed it (over the real HTTP response) strictly before the
    // server produced the *next* one. A buffered implementation would
    // fail this hard — every `client_arrivals[k]` would be >= the
    // server's *final* production instant, since nothing reaches the
    // client until the whole body is assembled.
    for k in 0..(COUNT as usize - 1) {
        assert!(
            client_arrivals[k] < server_produced_at[k + 1],
            "item {k} arrived at the client ({:?}) after the server had already produced \
             item {} ({:?}) — delivery is buffered, not incremental",
            client_arrivals[k],
            k + 1,
            server_produced_at[k + 1],
        );
    }
}
