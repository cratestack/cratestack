//! Shared by `subscription_cbor_seq_rpc.rs` and `subscription_cbor_seq_rest.rs`
//! (cratestack#1001): read an `application/cbor-seq` subscription body as
//! frames.

use cratestack::{CratestackContext, CratestackError, RequestContext, Value};
use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_core::CratestackCodec;
use cratestack_core::rpc::RpcFrameValue;
use cratestack_core::subscription_wire::ServerFrame;

/// What a body held: its frames, and how many bare `null` keep-alives sat
/// between them.
pub struct Body {
    pub frames: Vec<ServerFrame>,
    pub keepalives: usize,
}

/// Split `bytes` as a CBOR sequence; every item is a frame or a `null`.
pub fn read(bytes: &[u8]) -> Body {
    let mut decoder = CborSeqChunkDecoder::new();
    let items = decoder.feed_chunk(bytes).expect("a CBOR sequence");
    assert_eq!(
        decoder.pending_len(),
        0,
        "the body ends on an item boundary"
    );
    let mut body = Body {
        frames: Vec::new(),
        keepalives: 0,
    };
    for item in items {
        if item == [0xf6] {
            body.keepalives += 1;
        } else {
            body.frames
                .push(CborCodec.decode(&item).expect("a server frame"));
        }
    }
    body
}

/// The readings the fixtures' handlers send, `count` of them.
pub fn expected_id(n: i64) -> cratestack::uuid::Uuid {
    cratestack::uuid::Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_0000 + n as u128)
}

/// A `Reading` as a client reads it back.
#[derive(serde::Deserialize)]
struct Reading {
    id: cratestack::uuid::Uuid,
    data: Vec<u8>,
    amount: cratestack_core::RustDecimal,
    n: i64,
}

/// `frame` must be the `next` of reading `n`: its `Uuid` a CBOR byte string
/// (16 bytes, not text), and every field lossless once decoded.
pub fn assert_reading(frame: &ServerFrame, id: u64, n: i64) {
    assert_eq!(frame.id, id);
    let Some(next @ RpcFrameValue::Object(fields)) = &frame.next else {
        panic!("not a `next` object: {frame:?}");
    };
    assert_eq!(
        fields["id"],
        RpcFrameValue::Bytes(expected_id(n).as_bytes().to_vec()),
        "a Uuid is a 16-byte string"
    );
    let reading: Reading = next.decode_with(&CborCodec).expect("a Reading");
    assert_eq!(reading.id, expected_id(n));
    assert_eq!(reading.data, vec![0, 1, 255, n as u8]);
    assert_eq!(reading.amount.to_string(), format!("{n}.50"));
    assert_eq!(reading.n, n);
}

/// `x-caller` authenticates as principal 1; no header is anonymous.
#[derive(Clone)]
pub struct Caller;

impl cratestack::AuthProvider for Caller {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        core::future::ready(Ok(if request.headers.contains_key("x-caller") {
            CratestackContext::authenticated([("id".to_owned(), Value::Int(1))])
        } else {
            CratestackContext::anonymous()
        }))
    }
}
