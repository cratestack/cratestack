//! What both transports' tests share: read a cbor-seq body, and assert the
//! one `SeqNote` event it must carry.

use std::time::Duration;

use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{CratestackCodec, ModelEvent};
use futures_util::StreamExt;

pub const TABLE: [&str; 3] = [
    "DROP TABLE IF EXISTS seq_notes",
    "CREATE TABLE seq_notes (id UUID PRIMARY KEY, amount NUMERIC(38, 8) NOT NULL, body TEXT NOT NULL)",
    "DROP TABLE IF EXISTS cratestack_event_outbox",
];

/// The row, as a client with `Uuid` and decimal text fields reads it.
#[derive(Debug, serde::Deserialize)]
pub struct Row {
    pub id: uuid::Uuid,
    pub amount: String,
    pub body: String,
}

/// Every byte of the body within `window`; the stream never ends on its own.
pub async fn raw_body(response: cratestack::axum::response::Response, window: Duration) -> Vec<u8> {
    let mut body = response.into_body().into_data_stream();
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = tokio::time::timeout(window, body.next()).await {
        bytes.extend_from_slice(&chunk.expect("body chunk"));
    }
    bytes
}

/// The CBOR items of a cbor-seq body, whole.
fn items(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut decoder = CborSeqChunkDecoder::new();
    let items = decoder.feed_chunk(bytes).expect("a CBOR sequence");
    assert_eq!(decoder.pending_len(), 0, "no partial frame");
    items.into_iter().map(|item| item.to_vec()).collect()
}

/// The frames of a cbor-seq body.
pub fn frames_of(bytes: &[u8]) -> Vec<ServerFrame> {
    items(bytes)
        .iter()
        .map(|item| CborCodec.decode(item).expect("a server frame"))
        .collect()
}

/// Every frame within `window`.
pub async fn frames(
    response: cratestack::axum::response::Response,
    window: Duration,
) -> Vec<ServerFrame> {
    frames_of(&raw_body(response, window).await)
}

/// The committed hex body the TypeScript tests read
/// (`cratestack-client-typescript/tests/js/subscriptions/cbor-seq-model-feed.hex`)
/// must still be what the server writes for the same model and row. Compared
/// as CBOR values with the per-run `event_id` and `occurred_at` masked.
pub fn assert_matches_committed_fixture(live: &[u8]) {
    const PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cratestack-client-typescript/tests/js/subscriptions/cbor-seq-model-feed.hex"
    );
    let hex: String = std::fs::read_to_string(PATH)
        .expect("the committed fixture")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let hex = hex.trim();
    let committed: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex"))
        .collect();
    let mask = |bytes: &[u8]| -> Vec<RpcFrameValue> {
        items(bytes)
            .iter()
            .map(|item| {
                let mut frame: RpcFrameValue = CborCodec.decode(item).expect("a frame");
                if let RpcFrameValue::Object(map) = &mut frame
                    && let Some(RpcFrameValue::Object(event)) = map.get_mut("next")
                {
                    for field in ["event_id", "occurred_at"] {
                        event.insert(field.to_owned(), RpcFrameValue::Null);
                    }
                }
                frame
            })
            .collect()
    };
    // The fixture stops without a terminal frame, as the live capture does.
    let live_hex: String = live.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        mask(live),
        mask(&committed),
        "the server's cbor-seq body for `SeqNote` no longer matches the committed fixture \
         tests/js/subscriptions/cbor-seq-model-feed.hex (the TypeScript tests read it). \
         Regenerate: replace the hex line of that file (keep the `#` header) with the live \
         body below, then run the TypeScript suites that read it (`subscriptions_runtime`, \
         `subscription_cbor_seq_rest_runtime`).\nlive body: {live_hex}"
    );
}

/// A refusal before the stream: JSON, its own status, and `Vary: Accept`
/// like every subscribe response, so a cache keeps it apart from the other
/// binding's answer to the same URL.
pub fn assert_refusal(response: &cratestack::axum::response::Response, status: u16) {
    assert_eq!(response.status().as_u16(), status);
    assert_eq!(
        response.headers()[cratestack::axum::http::header::CONTENT_TYPE],
        "application/json"
    );
    assert_vary_accept_once(response);
}

/// Exactly one `Vary` header, whose value is `Accept`: a second append would
/// still pass a first-value-only check.
pub fn assert_vary_accept_once(response: &cratestack::axum::response::Response) {
    let values: Vec<_> = response
        .headers()
        .get_all(cratestack::axum::http::header::VARY)
        .iter()
        .collect();
    assert_eq!(values.len(), 1, "Vary must appear once: {values:?}");
    assert_eq!(values[0], "Accept");
}

/// `ready`, then one `next` whose `event_id` and `data.id` are 16-byte
/// strings (not the 36-character text SSE carries), and whose `Decimal`
/// is its exact text.
pub fn assert_one_event(frames: &[ServerFrame], row_id: uuid::Uuid) {
    assert_eq!(frames.len(), 2, "ready, then the created note: {frames:?}");
    assert!(frames[0].ready.is_some());
    assert_eq!((frames[0].id, frames[1].id), (1, 2));
    let next = frames[1].next.clone().expect("a `next` frame");
    let RpcFrameValue::Object(item) = &next else {
        panic!("a model event is a map, got {next:?}");
    };
    let RpcFrameValue::Bytes(event_id) = &item["event_id"] else {
        panic!(
            "event_id must be a 16-byte string over cbor-seq: {:?}",
            item["event_id"]
        );
    };
    assert_eq!(event_id.len(), 16);
    let RpcFrameValue::Object(data) = &item["data"] else {
        panic!("data is a map, got {:?}", item["data"]);
    };
    assert_eq!(data["id"], RpcFrameValue::Bytes(row_id.as_bytes().to_vec()));
    assert_eq!(
        data["amount"],
        RpcFrameValue::String("12.50000000".to_owned()),
        "a Decimal is its exact text (NUMERIC(38, 8) pads to 8 places)"
    );

    // And a typed client reads the whole event: the right Uuids, the row.
    let event: ModelEvent<Row> = next
        .decode_with(&CborCodec)
        .expect("the event decodes as a ModelEvent");
    assert_eq!(event.event_id.as_bytes(), event_id.as_slice());
    assert!(!event.event_id.is_nil());
    assert_eq!(event.model, "SeqNote");
    assert_eq!((event.data.id, event.data.body.as_str()), (row_id, "hello"));
    assert_eq!(event.data.amount, "12.50000000");
}
