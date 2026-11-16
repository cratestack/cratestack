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

/// Every frame within `window`; the stream never ends on its own.
pub async fn frames(
    response: cratestack::axum::response::Response,
    window: Duration,
) -> Vec<ServerFrame> {
    let mut body = response.into_body().into_data_stream();
    let mut decoder = CborSeqChunkDecoder::new();
    let mut frames = Vec::new();
    while let Ok(Some(chunk)) = tokio::time::timeout(window, body.next()).await {
        for item in decoder.feed_chunk(&chunk.expect("body chunk")).unwrap() {
            frames.push(CborCodec.decode(&item).expect("a server frame"));
        }
    }
    assert_eq!(decoder.pending_len(), 0, "no partial frame");
    frames
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
    assert_eq!(
        response.headers()[cratestack::axum::http::header::VARY],
        "Accept"
    );
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
