//! Genuinely incremental `application/cbor-seq` response encoding for
//! `@stream`-marked procedures (cratestack#283). Counterpart to
//! [`super::internal::encode_cbor_sequence_response`], which stays
//! exactly as it was for buffered (non-`@stream`) `T[]` procedures —
//! this module is additive, not a replacement; see
//! `crate::transport::encode_sequence::encode_transport_stream_result_with_status_for`
//! for where the two paths fork.
//!
//! Wire contract: `docs/design/rpc-transport.md` §3.3. Each item is
//! encoded and pushed onto the `axum::body::Body` stream as it's
//! produced — no `Vec<u8>` is ever fully materialized here. A mid-stream
//! `Err` (from the domain stream, or from a codec encode failure) is
//! translated to the CBOR-tagged error sentinel
//! ([`cratestack_core::RPC_STREAM_ERROR_TAG`]) as the final item; the
//! body then ends. The HTTP status is always the caller-supplied
//! `status` (200 in practice) — by the time any body byte can be
//! written, the status line is already committed, so a mid-stream
//! failure can never change it.

use std::pin::Pin;

use axum::body::{Body, Bytes};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use cratestack_core::rpc::{RPC_STREAM_ERROR_TAG, RpcErrorBody};
use cratestack_core::{CoolCodec, CoolError};
use futures_util::Stream;
use futures_util::stream::{self, StreamExt};
use serde::Serialize;

use super::CBOR_SEQUENCE_CONTENT_TYPE;
use super::http_transport::CborCodecMarker;
use crate::transport::STREAM_RESPONSE_HEADER;

#[cfg(test)]
mod tests;

pub(crate) fn encode_cbor_sequence_stream_response<C, T, S>(
    codec: C,
    status: StatusCode,
    items: S,
) -> Result<Response, CoolError>
where
    C: CoolCodec + Send + 'static,
    T: Serialize + Send + 'static,
    S: Stream<Item = Result<T, CoolError>> + Send + 'static,
{
    if C::CONTENT_TYPE != CborCodecMarker::CONTENT_TYPE {
        return Err(CoolError::NotAcceptable(
            "cbor-seq requires a CBOR codec".to_owned(),
        ));
    }

    let mut response = Response::new(Body::from_stream(encode_items_stream(codec, items)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(CBOR_SEQUENCE_CONTENT_TYPE),
    );
    // Internal-only signal (never part of the documented wire contract)
    // so response-buffering middleware — today just `IdempotencyService`
    // — can tell a genuinely incremental body apart from an ordinary
    // buffered `application/cbor-seq` response and bypass buffering
    // instead of silently re-collecting a partial stream. See
    // `crate::idempotency::service`.
    response.headers_mut().insert(
        STREAM_RESPONSE_HEADER,
        HeaderValue::from_static("incremental"),
    );
    Ok(response)
}

/// Adapts a `Stream<Item = Result<T, CoolError>>` into
/// `Stream<Item = Result<Bytes, Infallible>>`: encode each successful
/// item, or — on the first `Err` (from the domain stream OR a codec
/// encode failure) — emit the tag-48900 error sentinel and stop, never
/// polling the source again afterward. This is what makes "a stream
/// that emits a tag-48900 item never resumes normal output" (§3.3) true
/// regardless of what the underlying `S` would have produced next.
///
/// Boxing `items` (`Pin<Box<S>>`) rather than pinning it in place keeps
/// the `stream::unfold` accumulator `Unpin`, which is what lets a single
/// `async move` closure poll it across yields without `unsafe`; `S`
/// itself (typically an `async-stream`-generated state machine) is not
/// guaranteed `Unpin`.
fn encode_items_stream<C, T, S>(
    codec: C,
    items: S,
) -> impl Stream<Item = Result<Bytes, std::convert::Infallible>> + Send
where
    C: CoolCodec + Send + 'static,
    T: Serialize + Send + 'static,
    S: Stream<Item = Result<T, CoolError>> + Send + 'static,
{
    struct State<C, S> {
        codec: C,
        source: Pin<Box<S>>,
        done: bool,
    }

    let initial = State {
        codec,
        source: Box::pin(items),
        done: false,
    };

    stream::unfold(initial, |mut state| async move {
        if state.done {
            return None;
        }
        match state.source.next().await {
            Some(Ok(value)) => match state.codec.encode(&value) {
                Ok(bytes) => Some((Ok(Bytes::from(bytes)), state)),
                Err(encode_error) => {
                    let sentinel = encode_error_sentinel(&state.codec, &encode_error);
                    state.done = true;
                    Some((Ok(Bytes::from(sentinel)), state))
                }
            },
            Some(Err(error)) => {
                let sentinel = encode_error_sentinel(&state.codec, &error);
                state.done = true;
                Some((Ok(Bytes::from(sentinel)), state))
            }
            None => None,
        }
    })
}

/// `Tag(RPC_STREAM_ERROR_TAG, RpcErrorBody-as-CBOR-map)` — see
/// `docs/design/rpc-transport.md` §3.3. The tag header is written
/// directly via `minicbor::Encoder` (writes to `Vec<u8>` are
/// `Infallible`, so this can't fail); the map payload reuses the same
/// codec every ordinary item goes through, so `RpcErrorBody`'s field
/// encoding stays consistent with the rest of the wire format.
fn encode_error_sentinel<C: CoolCodec>(codec: &C, error: &CoolError) -> Vec<u8> {
    let mut bytes = Vec::new();
    minicbor::Encoder::new(&mut bytes)
        .tag(minicbor::data::Tag::new(RPC_STREAM_ERROR_TAG))
        .expect("writing a CBOR tag header to a Vec<u8> is infallible");
    let body = RpcErrorBody::from_cool(error);
    match codec.encode(&body) {
        Ok(encoded) => bytes.extend(encoded),
        Err(_) => {
            // `RpcErrorBody` is a plain struct of `String`/`Option` — if
            // even that fails to encode, the codec itself is broken.
            // Fall back to an empty map so the sentinel is still a
            // structurally valid `Tag(48900, {})` rather than a
            // truncated tag header with no payload at all.
            bytes.extend([0xa0]); // CBOR empty map (major type 5, len 0)
        }
    }
    bytes
}
