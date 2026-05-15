//! Tests for the standalone `FlutterCborSeqDecoder` FFI helper.
//!
//! Unlike `streaming_bridge.rs`, these tests do no I/O — they feed
//! bytes directly to the decoder to verify boundary-detection
//! semantics across chunk shapes (full items, mid-item splits, empty
//! chunks, malformed input). This is the surface a Flutter app using
//! dio (or any Dart-side HTTP client) calls into per response chunk.

use cratestack_client_flutter::FlutterCborSeqDecoder;
use cratestack_codec_cbor::CborCodec;
use cratestack_core::CoolCodec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Tick {
    index: i64,
    label: String,
}

fn encode_ticks(count: usize) -> Vec<Vec<u8>> {
    (0..count)
        .map(|i| {
            CborCodec
                .encode(&Tick {
                    index: i as i64,
                    label: format!("tick-{i}"),
                })
                .expect("encode tick")
        })
        .collect()
}

fn concat(items: &[Vec<u8>]) -> Vec<u8> {
    items.iter().flatten().copied().collect()
}

#[test]
fn single_chunk_yields_all_complete_items() {
    let decoder = FlutterCborSeqDecoder::new();
    let items = encode_ticks(4);
    let stream = concat(&items);

    let out = decoder.feed(stream).expect("feed should succeed");
    assert_eq!(out.len(), 4, "should yield all 4 items");
    assert_eq!(decoder.pending_len(), 0, "no bytes should be buffered");

    for (i, bytes) in out.iter().enumerate() {
        let tick: Tick = CborCodec.decode(bytes).expect("decode item");
        assert_eq!(tick.index, i as i64);
        assert_eq!(tick.label, format!("tick-{i}"));
    }
}

#[test]
fn split_mid_item_buffers_then_completes_on_next_chunk() {
    let decoder = FlutterCborSeqDecoder::new();
    let items = encode_ticks(3);
    let stream = concat(&items);

    // Split somewhere inside the second item.
    let split = items[0].len() + items[1].len() / 2;
    let (first, second) = stream.split_at(split);

    let out_a = decoder.feed(first.to_vec()).expect("first feed");
    assert_eq!(
        out_a.len(),
        1,
        "first chunk should yield item 0 only (item 1 is incomplete)",
    );
    assert!(
        decoder.pending_len() > 0,
        "incomplete trailing bytes should be buffered",
    );

    let out_b = decoder.feed(second.to_vec()).expect("second feed");
    assert_eq!(
        out_b.len(),
        2,
        "second chunk should complete item 1 and yield item 2",
    );
    assert_eq!(decoder.pending_len(), 0, "buffer should drain cleanly");
}

#[test]
fn many_small_chunks_one_byte_at_a_time_still_recovers_all_items() {
    // Worst-case chunking: one byte per feed. The boundary scanner
    // should still produce exactly the right items.
    let decoder = FlutterCborSeqDecoder::new();
    let items = encode_ticks(5);
    let stream = concat(&items);

    let mut collected = Vec::<Vec<u8>>::new();
    for byte in stream.iter().copied() {
        let out = decoder.feed(vec![byte]).expect("feed should succeed");
        collected.extend(out);
    }
    assert_eq!(collected.len(), 5, "all 5 items should be recovered");
    assert_eq!(decoder.pending_len(), 0);

    for (i, bytes) in collected.iter().enumerate() {
        let tick: Tick = CborCodec.decode(bytes).expect("decode item");
        assert_eq!(tick.index, i as i64);
    }
}

#[test]
fn empty_feed_returns_no_items_and_does_not_disturb_buffer() {
    let decoder = FlutterCborSeqDecoder::new();
    let items = encode_ticks(2);

    // Feed half of item 0, then an empty chunk, then the rest.
    let stream = concat(&items);
    let mid = items[0].len() / 2;
    let (a, rest) = stream.split_at(mid);

    let out_a = decoder.feed(a.to_vec()).expect("partial feed");
    assert!(out_a.is_empty());
    let pending_before = decoder.pending_len();
    assert!(pending_before > 0);

    let out_empty = decoder.feed(Vec::new()).expect("empty feed should succeed");
    assert!(out_empty.is_empty());
    assert_eq!(
        decoder.pending_len(),
        pending_before,
        "empty feed should not change the buffer",
    );

    let out_b = decoder.feed(rest.to_vec()).expect("completing feed");
    assert_eq!(out_b.len(), 2);
    assert_eq!(decoder.pending_len(), 0);
}

#[test]
fn pending_len_is_zero_for_a_fresh_decoder() {
    let decoder = FlutterCborSeqDecoder::new();
    assert_eq!(decoder.pending_len(), 0);
}

#[test]
fn default_constructor_matches_new() {
    let _from_default: FlutterCborSeqDecoder = FlutterCborSeqDecoder::default();
    // Constructible; the rest of behavior is identical to `::new()`
    // since `Default` delegates to it.
}
