use axum::body::to_bytes;
use axum::http::header;
use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;
use cratestack_core::{CratestackCodec, CratestackError, SubscriptionStream};
use futures_util::stream;

use super::encode_subscription_cbor_seq_response;

type Item = Result<RpcFrameValue, CratestackError>;

/// The response's whole body as items of the sequence.
async fn items_of(items: Vec<Item>) -> (String, Vec<Vec<u8>>) {
    let stream: SubscriptionStream<RpcFrameValue> = Box::pin(stream::iter(items));
    let response = encode_subscription_cbor_seq_response(stream);
    let content_type = response.headers()[header::CONTENT_TYPE]
        .to_str()
        .unwrap()
        .to_owned();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let mut decoder = CborSeqChunkDecoder::new();
    let items = decoder.feed_chunk(&bytes).unwrap();
    assert_eq!(decoder.pending_len(), 0, "no partial item at the end");
    (content_type, items)
}

fn frame(bytes: &[u8]) -> ServerFrame {
    CborCodec.decode(bytes).expect("a server frame")
}

#[tokio::test]
async fn ready_comes_first_then_each_item_then_end() {
    let (content_type, items) = items_of(vec![
        Ok(RpcFrameValue::Bytes(vec![0xab; 16])),
        Ok(RpcFrameValue::Null),
    ])
    .await;
    assert_eq!(content_type, "application/cbor-seq");
    assert_eq!(items.len(), 4);
    let ready = frame(&items[0]);
    assert_eq!((ready.id, ready.ready.is_some()), (1, true));
    let first = frame(&items[1]);
    assert_eq!(first.id, 2);
    assert_eq!(first.next, Some(RpcFrameValue::Bytes(vec![0xab; 16])));
    let second = frame(&items[2]);
    assert_eq!((second.id, second.next), (3, Some(RpcFrameValue::Null)));
    let end = frame(&items[3]);
    assert_eq!(
        (end.id, end.end.is_some(), end.err.is_none()),
        (4, true, true)
    );
}

#[tokio::test]
async fn an_error_is_the_terminal_frame_and_nothing_follows() {
    let (_, items) = items_of(vec![
        Ok(RpcFrameValue::Bool(true)),
        Err(CratestackError::Unavailable(
            "subscription lagged".to_owned(),
        )),
        Ok(RpcFrameValue::Bool(false)),
    ])
    .await;
    assert_eq!(items.len(), 3, "ready, one item, the error");
    let err = frame(&items[2]).err.expect("an err frame");
    assert_eq!(err.code, "unavailable");
    assert_eq!(err.message, "subscription lagged");
}

#[tokio::test(start_paused = true)]
async fn a_quiet_subscription_gets_a_cbor_null_keepalive() {
    use futures_util::StreamExt;

    let quiet: SubscriptionStream<RpcFrameValue> = Box::pin(stream::pending());
    let mut body = encode_subscription_cbor_seq_response(quiet)
        .into_body()
        .into_data_stream();
    let ready = body.next().await.unwrap().unwrap();
    assert!(frame(&ready).ready.is_some());
    let keepalive = body.next().await.unwrap().unwrap();
    assert_eq!(&keepalive[..], &[0xf6]);
    let again = body.next().await.unwrap().unwrap();
    assert_eq!(&again[..], &[0xf6], "one per quiet interval");
}
