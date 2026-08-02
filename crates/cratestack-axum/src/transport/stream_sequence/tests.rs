use axum::http::header;
use cratestack_codec_cbor::CborCodec;

use super::*;

fn tick(n: u32) -> serde_json::Value {
    serde_json::json!({ "n": n })
}

#[tokio::test]
async fn encodes_each_item_and_stops_cleanly_at_end_of_stream() {
    let items = stream::iter(vec![Ok(tick(0)), Ok(tick(1)), Ok(tick(2))]);
    let response = encode_cbor_sequence_stream_response(CborCodec, StatusCode::OK, items).unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok()),
        Some(CBOR_SEQUENCE_CONTENT_TYPE)
    );
    assert!(
        response
            .extensions()
            .get::<StreamedResponseMarker>()
            .is_some(),
        "incremental responses must carry the StreamedResponseMarker extension"
    );
    assert!(
        !response.headers().contains_key("x-cratestack-stream"),
        "the stream marker must never appear as a header (it would leak to real clients) — \
         see StreamedResponseMarker's doc comment"
    );

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let values: Vec<serde_json::Value> = decode_cbor_seq(&bytes);
    assert_eq!(values, vec![tick(0), tick(1), tick(2)]);
}

#[tokio::test]
async fn mid_stream_error_becomes_the_final_tagged_item() {
    let items = stream::iter(vec![
        Ok(tick(0)),
        Ok(tick(1)),
        Err(CoolError::Internal("boom".to_owned())),
    ]);
    let response = encode_cbor_sequence_stream_response(CborCodec, StatusCode::OK, items).unwrap();
    // Status is still 200 — the failure is in-band, not a status code.
    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let items = split_cbor_seq_items(&bytes);
    assert_eq!(items.len(), 3, "two ok items + one tagged error item");
    assert!(
        starts_with_error_tag_header(&items[2]),
        "final item must start with the tag-48900 header"
    );
    assert!(
        !starts_with_error_tag_header(&items[0]) && !starts_with_error_tag_header(&items[1]),
        "successful items must not carry the error tag"
    );
}

#[tokio::test]
async fn nothing_after_the_error_item_even_if_source_would_yield_more() {
    // A source that, if fully drained, would yield a fourth item — the
    // encoder must never poll it again after the `Err`.
    let items = stream::iter(vec![
        Ok(tick(0)),
        Err(CoolError::Internal("boom".to_owned())),
        Ok(tick(2)),
    ]);
    let response = encode_cbor_sequence_stream_response(CborCodec, StatusCode::OK, items).unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let items = split_cbor_seq_items(&bytes);
    assert_eq!(items.len(), 2, "must stop right after the error sentinel");
    assert!(starts_with_error_tag_header(&items[1]));
}

#[tokio::test]
async fn empty_stream_produces_an_empty_body() {
    let items: stream::Iter<std::vec::IntoIter<Result<serde_json::Value, CoolError>>> =
        stream::iter(vec![]);
    let response = encode_cbor_sequence_stream_response(CborCodec, StatusCode::OK, items).unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert!(bytes.is_empty());
}

#[tokio::test]
async fn non_cbor_codec_is_rejected() {
    use cratestack_codec_json::JsonCodec;
    let items = stream::iter(vec![Ok(tick(0))]);
    let result = encode_cbor_sequence_stream_response(JsonCodec, StatusCode::OK, items);
    assert!(result.is_err());
}

#[test]
fn error_sentinel_tag_header_matches_rfc_8949_two_byte_form() {
    // Tag 48900 = 0xBF04, in [0x100, 0xffff] -> 2-byte argument form:
    // major type 6 (0xC0) | additional info 25 (0x19) = 0xD9, then the
    // tag number big-endian.
    let sentinel = encode_error_sentinel(&CborCodec, &CoolError::Internal("x".to_owned()));
    assert_eq!(&sentinel[..3], &[0xD9, 0xBF, 0x04]);
}

fn starts_with_error_tag_header(item: &[u8]) -> bool {
    item.starts_with(&[0xD9, 0xBF, 0x04])
}

/// Boundary-scan raw cbor-seq bytes into per-item byte ranges,
/// mirroring `CborSeqChunkDecoder` (`cratestack-client-rust`) at a scale
/// small enough not to need that crate as a dev-dependency here.
fn split_cbor_seq_items(bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut items = Vec::new();
    let mut offset = 0usize;
    while offset < bytes.len() {
        let mut decoder = minicbor::Decoder::new(&bytes[offset..]);
        decoder.skip().expect("each cbor-seq item should decode");
        let len = decoder.position();
        assert!(len > 0, "decoder must make progress");
        items.push(bytes[offset..offset + len].to_vec());
        offset += len;
    }
    items
}

fn decode_cbor_seq<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Vec<T> {
    split_cbor_seq_items(bytes)
        .into_iter()
        .map(|item| minicbor_serde::from_slice(&item).expect("item should decode"))
        .collect()
}
