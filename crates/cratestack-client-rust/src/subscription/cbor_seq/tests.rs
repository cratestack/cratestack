use cratestack_core::CratestackError;
use cratestack_core::rpc::RpcFrameValue;

use super::*;

fn encode(frame: &ServerFrame) -> Vec<u8> {
    CborCodec.encode(frame).unwrap()
}

fn body(frames: &[ServerFrame]) -> Vec<u8> {
    frames.iter().flat_map(encode).collect()
}

async fn collect(chunks: Vec<Vec<u8>>, limit: usize) -> Vec<Result<ServerFrame, ClientError>> {
    let chunks: Vec<_> = chunks.into_iter().map(|c| Ok(Bytes::from(c))).collect();
    frames(futures_util::stream::iter(chunks), limit)
        .collect()
        .await
}

#[tokio::test]
async fn frames_split_at_every_byte_are_all_delivered_in_order() {
    let wire = body(&[
        ServerFrame::ready(1),
        ServerFrame::next(2, RpcFrameValue::Bytes(vec![9; 40])),
        ServerFrame::next(3, RpcFrameValue::String("café".to_owned())),
        ServerFrame::end(4),
    ]);
    let one_byte_chunks = wire.iter().map(|byte| vec![*byte]).collect();
    let out = collect(one_byte_chunks, 1 << 20).await;
    let out: Vec<_> = out.into_iter().map(Result::unwrap).collect();
    assert_eq!(out.len(), 4);
    assert!(out[0].ready.is_some());
    assert_eq!(out[1].next, Some(RpcFrameValue::Bytes(vec![9; 40])));
    assert_eq!(out[2].next, Some(RpcFrameValue::String("café".to_owned())));
    assert!(out[3].end.is_some());
}

#[tokio::test]
async fn several_frames_in_one_chunk_are_all_delivered() {
    let wire = body(&[ServerFrame::ready(1), ServerFrame::end(2)]);
    let out = collect(vec![wire], 1 << 20).await;
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn keepalive_nulls_are_skipped_between_and_inside_chunks() {
    let mut wire = vec![0xf6];
    wire.extend(encode(&ServerFrame::ready(1)));
    wire.extend([0xf6, 0xf6]);
    wire.extend(encode(&ServerFrame::end(2)));
    let (a, b) = wire.split_at(3);
    let out = collect(vec![a.to_vec(), b.to_vec(), vec![0xf6]], 1 << 20).await;
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].as_ref().unwrap().ready.is_some());
    assert!(out[1].as_ref().unwrap().end.is_some());
}

#[tokio::test]
async fn a_null_item_value_is_not_a_keepalive() {
    let wire = body(&[ServerFrame::next(2, RpcFrameValue::Null)]);
    let out = collect(vec![wire], 1 << 20).await;
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].as_ref().unwrap().next, Some(RpcFrameValue::Null));
}

#[tokio::test]
async fn a_body_that_ends_inside_a_frame_is_an_error_not_a_quiet_end() {
    let wire = body(&[ServerFrame::ready(1), ServerFrame::end(2)]);
    let cut = wire.len() - 2;
    let out = collect(vec![wire[..cut].to_vec()], 1 << 20).await;
    assert_eq!(out.len(), 2, "{out:?}");
    assert!(out[0].is_ok());
    let Err(ClientError::InvalidResponse(message)) = &out[1] else {
        panic!("expected a truncation error: {out:?}");
    };
    assert!(message.contains("middle of a frame"), "{message}");
}

#[tokio::test]
async fn a_frame_exactly_at_the_cap_is_accepted_and_one_byte_over_is_refused() {
    let frame = encode(&ServerFrame::next(2, RpcFrameValue::Bytes(vec![1; 64])));
    let at = collect(vec![frame.clone()], frame.len()).await;
    assert!(at[0].is_ok(), "{at:?}");
    // A second frame follows in the same chunk: it must not arrive.
    let mut wire = frame.clone();
    wire.extend(encode(&ServerFrame::end(3)));
    let over = collect(vec![wire], frame.len() - 1).await;
    assert_eq!(over.len(), 1, "{over:?}");
    assert!(matches!(
        over[0],
        Err(ClientError::EventTooLarge { limit }) if limit == frame.len() - 1
    ));
}

#[tokio::test]
async fn an_oversize_frame_split_across_chunks_is_refused_before_it_completes() {
    let frame = encode(&ServerFrame::next(2, RpcFrameValue::Bytes(vec![1; 500])));
    let chunks = frame.chunks(40).map(<[u8]>::to_vec).collect();
    let out = collect(chunks, 100).await;
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(matches!(
        out[0],
        Err(ClientError::EventTooLarge { limit: 100 })
    ));
}

/// A byte string that declares 4 GiB, then trickles forever. The stream
/// yields to the runtime between chunks, so the timeout fires if the reader
/// never stops.
#[tokio::test]
async fn an_item_that_never_ends_is_cut_off_at_the_cap() {
    let endless = futures_util::stream::unfold(true, |first| async move {
        tokio::task::yield_now().await;
        let chunk = if first {
            Bytes::from_static(&[0x5a, 0xff, 0xff, 0xff, 0xff])
        } else {
            Bytes::from_static(&[7; 100])
        };
        Some((Ok(chunk), false))
    });
    let mut stream = Box::pin(frames(Box::pin(endless), 1024));
    let first = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
        .await
        .expect("the reader must stop buffering, not hang");
    assert!(matches!(
        first,
        Some(Err(ClientError::EventTooLarge { limit: 1024 }))
    ));
    assert!(stream.next().await.is_none(), "nothing after the refusal");
}

#[tokio::test]
async fn bytes_that_are_not_a_frame_are_an_error() {
    // A CBOR text string, not a frame map.
    let out = collect(vec![vec![0x63, b'a', b'b', b'c']], 1 << 20).await;
    assert!(matches!(out[0], Err(ClientError::InvalidResponse(_))));
}

#[tokio::test]
async fn an_err_frame_keeps_its_code() {
    let wire = body(&[ServerFrame::err(
        2,
        &CratestackError::Unavailable("lagged".to_owned()),
    )]);
    let out = collect(vec![wire], 1 << 20).await;
    let err = out[0].as_ref().unwrap().err.clone().unwrap();
    assert_eq!(err.code, "unavailable");
}

#[tokio::test]
async fn an_item_that_is_not_a_map_is_an_invalid_response() {
    // A bare integer, a text string and a byte string: none is a `ServerFrame`
    // (a CBOR map), and none may read as one.
    for item in [vec![0x05], vec![0x61, b'x'], vec![0x41, 0x01]] {
        let mut wire = encode(&ServerFrame::ready(1));
        wire.extend(&item);
        let out = collect(vec![wire], 1 << 20).await;
        assert_eq!(out.len(), 2, "{item:?}: {out:?}");
        assert!(out[0].as_ref().unwrap().ready.is_some());
        assert!(
            matches!(&out[1], Err(ClientError::InvalidResponse(m)) if m.contains("subscription frame")),
            "{item:?}: {out:?}"
        );
    }
}
