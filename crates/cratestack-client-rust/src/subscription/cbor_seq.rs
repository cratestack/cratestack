//! A subscription over `application/cbor-seq`
//! (`docs/design/subscriptions-websocket.md` §6a.3, cratestack#1001): `GET`
//! the same route SSE uses, with `Accept: application/cbor-seq`. The body is
//! a CBOR sequence of server frames: the call resolves at `ready`, every
//! `next` is an item, and `end` or `err` is the last frame. A bare CBOR
//! `null` is a keep-alive and is skipped.
//!
//! Items decode the way a unary CBOR call does, so a `Uuid` is a byte string
//! and `Bytes` stay bytes. A body that stops without `end` or `err` — in the
//! middle of an item, or between two — is an error, not a quiet end: it is
//! the connection dropping, which a caller wants to hear about.

use bytes::Bytes;
use cratestack_codec_cbor::{CborCodec, CborSeqChunkDecoder};
use cratestack_core::CratestackCodec;
use cratestack_core::subscription_wire::ServerFrame;
use futures_util::{Stream, StreamExt};
use reqwest::Method;
use serde::de::DeserializeOwned;
use std::collections::VecDeque;

use super::frames::{cbor_item, rpc_error_to_client};
use super::{ClientSubscription, SubscriptionTarget};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

const ACCEPT: &str = "application/cbor-seq";

/// The keep-alive: a CBOR `null`.
const KEEPALIVE: &[u8] = &[0xf6];

pub(super) async fn open<C: HttpClientCodec, T: DeserializeOwned + Send + 'static>(
    client: &CratestackClient<C>,
    target: &SubscriptionTarget,
) -> Result<ClientSubscription<T>, ClientError> {
    let response = client
        .request_streamed_with_query_and_accept(
            Method::GET,
            &target.sse_path,
            None,
            target.sse_query.as_deref(),
            &[],
            ACCEPT,
        )
        .await?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !is_cbor_seq(content_type) {
        return Err(ClientError::InvalidResponse(format!(
            "expected a {ACCEPT} subscription body, got Content-Type `{content_type}`"
        )));
    }
    let mut frames = Box::pin(frames(
        response.bytes_stream(),
        client.config.max_sse_event_bytes,
    ));
    match frames.next().await {
        Some(Ok(frame)) if frame.ready.is_some() => {}
        Some(Ok(ServerFrame { err: Some(err), .. })) => return Err(rpc_error_to_client(err)),
        Some(Err(error)) => return Err(error),
        _ => {
            return Err(ClientError::InvalidResponse(
                "a subscription must start with a `ready` frame".to_owned(),
            ));
        }
    }
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    crate::task::spawn(async move {
        loop {
            let (item, last) = match frames.next().await {
                Some(Ok(ServerFrame {
                    next: Some(next), ..
                })) => (cbor_item(next), false),
                Some(Ok(ServerFrame { err: Some(err), .. })) => {
                    (Err(rpc_error_to_client(err)), true)
                }
                Some(Ok(frame)) if frame.end.is_some() => return,
                Some(Ok(_)) => continue,
                Some(Err(error)) => (Err(error), true),
                None => (Err(missing_terminal()), true),
            };
            let last = last || item.is_err();
            if tx.send(item).await.is_err() || last {
                return;
            }
        }
    });
    Ok(rx)
}

/// Whether a `Content-Type` is `application/cbor-seq`: parameters stripped,
/// compared without case.
fn is_cbor_seq(content_type: &str) -> bool {
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    media_type.eq_ignore_ascii_case(ACCEPT)
}

fn missing_terminal() -> ClientError {
    ClientError::InvalidResponse(
        "the subscription body ended without an `end` or `err` frame".to_owned(),
    )
}

/// The frames of a CBOR-sequence body, keep-alives skipped.
///
/// One frame may span at most `limit` bytes (the same cap, and the same
/// [`ClientError::EventTooLarge`], as an SSE event): checked on every item
/// and on the unfinished tail after every chunk, so an item that never ends
/// is caught as surely as one that arrives whole. Past it the stream yields
/// the error once, then ends and is dropped, which drops the connection;
/// nothing behind the oversized frame is delivered. A body that ends inside
/// an item yields an error, once. A clean end of body yields `None`.
pub(super) fn frames<S>(
    bytes: S,
    limit: usize,
) -> impl Stream<Item = Result<ServerFrame, ClientError>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    struct State<S> {
        bytes: S,
        decoder: CborSeqChunkDecoder,
        ready: VecDeque<Vec<u8>>,
    }
    let state = Some(State {
        bytes,
        decoder: CborSeqChunkDecoder::new(),
        ready: VecDeque::new(),
    });
    futures_util::stream::unfold(state, move |state| async move {
        let mut state = state?;
        loop {
            if let Some(item) = state.ready.pop_front() {
                if item.len() > limit {
                    return Some((Err(ClientError::EventTooLarge { limit }), None));
                }
                if item == KEEPALIVE {
                    continue;
                }
                let frame = CborCodec.decode::<ServerFrame>(&item).map_err(|error| {
                    ClientError::InvalidResponse(format!("subscription frame: {error}"))
                });
                return Some((frame, Some(state)));
            }
            // Only once every complete frame is delivered: the tail is what
            // the next chunk would extend.
            if state.decoder.pending_len() > limit {
                return Some((Err(ClientError::EventTooLarge { limit }), None));
            }
            match state.bytes.next().await {
                Some(Ok(chunk)) => match state.decoder.feed_chunk(&chunk) {
                    Ok(items) => state.ready.extend(items),
                    Err(error) => return Some((Err(error.into()), None)),
                },
                Some(Err(error)) => return Some((Err(error.into()), None)),
                None if state.decoder.pending_len() > 0 => {
                    let error = ClientError::InvalidResponse(
                        "the subscription body ended in the middle of a frame".to_owned(),
                    );
                    return Some((Err(error), None));
                }
                None => return None,
            }
        }
    })
}

#[cfg(test)]
mod tests;
