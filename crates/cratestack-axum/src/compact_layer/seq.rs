//! Compacting a `cbor-seq` body item by item.

use axum::body::{Body, Bytes};
use bytes::BytesMut;
use cratestack_codec_cbor::CborCodec;
use cratestack_codec_cbor::compact::cbor_to_compact;
use cratestack_core::compact::{KeyNode, KeyTables};
use cratestack_core::rpc::RPC_STREAM_ERROR_TAG;
use cratestack_core::{CratestackError, MAX_RESPONSE_REBUFFER_BYTES};
use futures_util::StreamExt;
use futures_util::stream::{self, BoxStream};
use minicbor::Decoder;
use std::io;
use std::sync::Arc;

use crate::transport::encode_error_sentinel;

type Source = BoxStream<'static, Result<Bytes, axum::Error>>;

struct State {
    source: Source,
    buffer: BytesMut,
    tables: Arc<KeyTables>,
    node: KeyNode,
    /// The rest of the body is forwarded as it is (malformed framing).
    raw: bool,
    done: bool,
}

/// How the front of a buffer starts.
enum Front {
    /// A whole item of this length; `error` is the tag-48900 item.
    Item { len: usize, error: bool },
    /// Not all of the item has arrived.
    Partial,
    /// Not CBOR.
    Malformed,
}

fn front(buffer: &[u8]) -> Front {
    let mut decoder = Decoder::new(buffer);
    let error = decoder
        .clone()
        .tag()
        .is_ok_and(|tag| tag.as_u64() == RPC_STREAM_ERROR_TAG);
    match decoder.skip() {
        Ok(()) => Front::Item {
            len: decoder.position(),
            error,
        },
        Err(e) if e.is_end_of_input() => Front::Partial,
        Err(_) => Front::Malformed,
    }
}

/// `body` with every item rewritten to integer keys under `node`.
pub(super) fn compact_sequence(body: Body, tables: Arc<KeyTables>, node: KeyNode) -> Body {
    let state = State {
        source: body.into_data_stream().boxed(),
        buffer: BytesMut::new(),
        tables,
        node,
        raw: false,
        done: false,
    };
    Body::from_stream(stream::unfold(state, next))
}

async fn next(mut state: State) -> Option<(Result<Bytes, io::Error>, State)> {
    loop {
        if state.done {
            return None;
        }
        if state.raw {
            return match state.source.next().await {
                Some(Ok(chunk)) => Some((Ok(chunk), state)),
                Some(Err(e)) => Some((Err(io::Error::other(e)), state)),
                None => None,
            };
        }
        match front(&state.buffer) {
            Front::Item { len, error } => {
                let item = state.buffer.split_to(len);
                let out = if error {
                    item.freeze()
                } else {
                    match cbor_to_compact(&state.tables, &state.node, &item) {
                        Ok(compact) => Bytes::from(compact),
                        Err(_) => {
                            state.done = true;
                            Bytes::from(encode_error_sentinel(
                                &CborCodec,
                                &CratestackError::Internal("compact the stream item".to_owned()),
                            ))
                        }
                    }
                };
                return Some((Ok(out), state));
            }
            Front::Malformed => {
                state.raw = true;
                let rest = state.buffer.split().freeze();
                return Some((Ok(rest), state));
            }
            Front::Partial => match state.source.next().await {
                Some(Ok(chunk)) => {
                    state.buffer.extend_from_slice(&chunk);
                    if state.buffer.len() > MAX_RESPONSE_REBUFFER_BYTES {
                        state.done = true;
                        let error = io::Error::other("stream item exceeds the response bound");
                        return Some((Err(error), state));
                    }
                }
                Some(Err(e)) => {
                    state.done = true;
                    return Some((Err(io::Error::other(e)), state));
                }
                None => {
                    state.done = true;
                    let rest = state.buffer.split().freeze();
                    return (!rest.is_empty()).then_some((Ok(rest), state));
                }
            },
        }
    }
}
