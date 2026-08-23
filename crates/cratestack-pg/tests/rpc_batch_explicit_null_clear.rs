//! cratestack#677 — decisive test for the Rust RPC batch client's
//! obsolete `strip_json_null_entries` workaround
//! (`crates/cratestack-client-rust/src/rpc/batch_call.rs`).
//!
//! Settles the question the issue leaves open: does the strip silently
//! drop an explicit `null` meaning "clear this nullable column" on a
//! batched `model.<Model>.update`? Two single-frame batches go through
//! the SAME production round trip (`BatchableCall::queue` +
//! `BatchBuilder::send`) — one carrying an explicit clear, one carrying
//! an untouched field — and the test asserts on the RAW HTTP request
//! bytes the mock server receives, not decoded values: an asymmetric
//! encoder/decoder pair can round-trip cleanly while still putting the
//! wrong thing on the wire.
//!
//! `Note` is deliberately pared down to a PK and a single nullable
//! field (see `tests/fixtures/rpc_batch_explicit_null_clear.cstack`) so
//! `note` is the only place a CBOR null (`0xf6`) can appear anywhere in
//! either request's bytes — no other `Option`-typed field in
//! `RpcRequest`/`RpcUpdateInput` to confuse the signal.
//!
//! Decisive check (quoted in the PR description): restoring
//! `strip_json_null_entries` on the `BatchableCall::new` call site makes
//! this test fail, because the strip recurses into the `patch` object
//! and removes `note`'s explicit-null entry exactly like it removes an
//! untouched field's — both frames would decode identically and the
//! clear would be silently lost.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, Response, StatusCode, header};
use axum::routing::post;
use cratestack::include_client_schema;
use cratestack_client_rust::{CborCodec, ClientConfig, CratestackClient};
use cratestack_core::CratestackCodec;

include_client_schema!("tests/fixtures/rpc_batch_explicit_null_clear.cstack");

#[tokio::test]
async fn explicit_null_clear_reaches_server_distinct_from_untouched() {
    let captured: Arc<Mutex<Vec<Vec<u8>>>> = Arc::new(Mutex::new(Vec::new()));
    let (base_url, _server) = spawn_capturing_server(captured.clone()).await;
    let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
    let client = cratestack_schema::client::Client::new(runtime);

    // Frame A: explicit clear — `note: Some(None)`, "clear this column".
    let mut clear_batch = client.batch();
    let h_clear = client
        .notes()
        .update(
            &1i64,
            &cratestack_schema::UpdateNoteInput { note: Some(None) },
        )
        .queue(&mut clear_batch);
    let mut clear_results = clear_batch
        .send()
        .await
        .expect("clear batch should round-trip at the HTTP envelope level");
    let cleared = clear_results
        .take(h_clear)
        .expect("clear frame should resolve");
    assert_eq!(cleared.id, 1);

    // Frame B: untouched — `note: None`, "the caller never mentioned this
    // field", must stay off the wire per #663.
    let mut untouched_batch = client.batch();
    let h_untouched = client
        .notes()
        .update(&1i64, &cratestack_schema::UpdateNoteInput { note: None })
        .queue(&mut untouched_batch);
    let mut untouched_results = untouched_batch
        .send()
        .await
        .expect("untouched batch should round-trip at the HTTP envelope level");
    let untouched = untouched_results
        .take(h_untouched)
        .expect("untouched frame should resolve");
    assert_eq!(untouched.id, 1);

    let bodies = captured.lock().expect("capture lock");
    assert_eq!(bodies.len(), 2, "expected exactly two captured requests");
    let clear_body = &bodies[0];
    let untouched_body = &bodies[1];

    // --- Decisive check #1: raw encoded bytes, not decoded values. ---
    // 0xf6 is CBOR null; 0x80 is the empty-array marker #657 fixed.
    assert!(
        clear_body.contains(&0xf6),
        "explicit clear must reach the wire as CBOR null (0xf6): {clear_body:02x?}"
    );
    assert!(
        !untouched_body.contains(&0xf6),
        "an untouched field must stay OFF the wire entirely (#663 contract), \
         not appear as CBOR null: {untouched_body:02x?}"
    );

    // --- Decisive check #2: what the server actually decodes the patch
    // into, using the exact generated `UpdateNoteInput` type (identical
    // codegen either role uses) — `Some(None)` drives a real
    // `SET note = NULL`; `None` omits the column from the SET clause
    // entirely and leaves it untouched. This is the load-bearing
    // assertion for the "silent data loss" claim in #677.
    let clear_frames: Vec<cratestack::rpc::RpcRequest> =
        CborCodec.decode(clear_body).expect("decode clear batch");
    let clear_patch: cratestack_schema::UpdateNoteInput =
        serde_json::from_value(clear_frames[0].input["patch"].clone())
            .expect("decode UpdateNoteInput");
    assert_eq!(
        clear_patch.note,
        Some(None),
        "server must observe an explicit clear as Some(None), not None (== untouched)"
    );

    let untouched_frames: Vec<cratestack::rpc::RpcRequest> = CborCodec
        .decode(untouched_body)
        .expect("decode untouched batch");
    let untouched_patch: cratestack_schema::UpdateNoteInput =
        serde_json::from_value(untouched_frames[0].input["patch"].clone())
            .expect("decode UpdateNoteInput");
    assert_eq!(
        untouched_patch.note, None,
        "an untouched field must decode to None (#663 contract)"
    );
}

fn cbor_response<T: serde::Serialize>(status: StatusCode, body: &T) -> Response<Body> {
    let bytes = CborCodec.encode(body).expect("encode body");
    Response::builder()
        .status(status)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/cbor"),
        )
        .body(Body::from(bytes))
        .expect("response builds")
}

/// A batch server that records every raw request body it receives (in
/// arrival order) before decoding anything, then answers with a canned
/// `model.Note.update` response so the client-side `.send()` resolves.
async fn spawn_capturing_server(
    captured: Arc<Mutex<Vec<Vec<u8>>>>,
) -> (url::Url, tokio::task::JoinHandle<()>) {
    let app = Router::new().route(
        "/rpc/batch",
        post(move |body: Bytes| {
            let captured = captured.clone();
            async move {
                captured.lock().expect("capture lock").push(body.to_vec());
                let requests: Vec<cratestack::rpc::RpcRequest> =
                    CborCodec.decode(&body).expect("decode batch frames");
                let responses: Vec<cratestack::rpc::RpcResponseFrame> = requests
                    .into_iter()
                    .map(|req| {
                        let note_value = serde_json::to_value(cratestack_schema::Note {
                            id: 1,
                            note: Some("whatever".to_owned()),
                        })
                        .expect("encode Note");
                        cratestack::rpc::RpcResponseFrame {
                            id: req.id,
                            output: Some(note_value),
                            error: None,
                        }
                    })
                    .collect();
                cbor_response(StatusCode::OK, &responses)
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let addr: SocketAddr = listener.local_addr().expect("listener has addr");
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("server should run");
    });
    (
        url::Url::parse(&format!("http://{addr}")).expect("base url parses"),
        handle,
    )
}
