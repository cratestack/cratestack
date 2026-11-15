//! A subscription over SSE (`docs/design/subscriptions-websocket.md` §7.1):
//! `GET` the route with the input as its query string; the call resolves at
//! `event: ready`, then every `event: message` is an item and one
//! `event: error` ends it. `: keepalive` comments are skipped.

use bytes::Bytes;
use cratestack_core::rpc::RpcErrorBody;
use futures_util::{Stream, StreamExt};
use reqwest::Method;
use serde::de::DeserializeOwned;

use super::frames::{item, rpc_error_to_client};
use super::{ClientSubscription, SubscriptionTarget};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::error::ClientError;

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
            "text/event-stream",
        )
        .await?;
    let mut events = Box::pin(events(
        response.bytes_stream(),
        client.config.max_sse_event_bytes,
    ));
    match events.next().await {
        Some(Ok(event)) if event.name == "ready" => {}
        Some(Ok(event)) if event.name == "error" => return Err(error_of(&event.data)),
        Some(Err(error)) => return Err(error),
        _ => {
            return Err(ClientError::InvalidResponse(
                "a subscription must start with `event: ready`".to_owned(),
            ));
        }
    }
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    crate::task::spawn(async move {
        while let Some(event) = events.next().await {
            let (item, last) = match event {
                Ok(event) if event.name == "message" => (next_of(&event.data), false),
                Ok(event) if event.name == "error" => (Err(error_of(&event.data)), true),
                Ok(_) => continue,
                Err(error) => (Err(error), true),
            };
            let last = last || item.is_err();
            if tx.send(item).await.is_err() || last {
                return;
            }
        }
    });
    Ok(rx)
}

fn next_of<T: DeserializeOwned>(data: &str) -> Result<T, ClientError> {
    #[derive(serde::Deserialize)]
    struct Next {
        next: serde_json::Value,
    }
    let frame: Next = serde_json::from_str(data)
        .map_err(|error| ClientError::InvalidResponse(format!("subscription frame: {error}")))?;
    item(frame.next)
}

fn error_of(data: &str) -> ClientError {
    #[derive(serde::Deserialize)]
    struct Err {
        err: RpcErrorBody,
    }
    match serde_json::from_str::<Err>(data) {
        Ok(frame) => rpc_error_to_client(frame.err),
        Err(error) => ClientError::InvalidResponse(format!("subscription error frame: {error}")),
    }
}

/// One server-sent event.
#[derive(Debug, PartialEq)]
pub(super) struct Event {
    pub(super) name: String,
    pub(super) data: String,
}

/// The events of an SSE body. A block with no `data:` (a comment, a
/// keepalive) yields nothing. The buffer holds raw bytes until a whole
/// block has arrived, so a multi-byte character or a `\r\n` split across
/// two transport chunks is decoded only once it is complete.
///
/// One event may span at most `limit` raw bytes as received, line endings
/// and the terminating blank line included (cratestack#1379); the
/// TypeScript and Dart readers count UTF-8 bytes after CRLF -> LF
/// normalisation instead, so they may accept up to one more byte per line.
/// The buffer is checked whenever it holds no complete block, so an endless
/// line with no newline is caught as surely as endless short ones. Past
/// the cap the stream yields [`ClientError::EventTooLarge`] once, then ends
/// and is dropped, which drops the connection; nothing buffered behind the
/// oversized event is delivered.
///
/// Parsing is linear: consumed events are skipped with an offset and the
/// buffer is compacted once per transport chunk, and each delimiter search
/// resumes where the previous one stopped.
pub(super) fn events<S>(bytes: S, limit: usize) -> impl Stream<Item = Result<Event, ClientError>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    // `start`: the consumed prefix of `buffer`. `scanned`: how far the
    // unconsumed part has been searched for a delimiter (absolute index), so
    // neither a trickle of tiny chunks nor a run of blank lines rescans.
    futures_util::stream::unfold(
        Some((bytes, Vec::<u8>::new(), 0usize, 0usize)),
        move |state| async move {
            let (mut bytes, mut buffer, mut start, mut scanned) = state?;
            loop {
                if let Some(end) = block_end(&buffer, scanned.max(start)) {
                    if end - start > limit {
                        return Some((Err(ClientError::EventTooLarge { limit }), None));
                    }
                    let block = &buffer[start..end];
                    start = end;
                    scanned = end;
                    let Ok(text) = std::str::from_utf8(block) else {
                        let error =
                            ClientError::InvalidResponse("SSE body is not UTF-8".to_owned());
                        return Some((Err(error), Some((bytes, buffer, start, scanned))));
                    };
                    if let Some(event) = parse_block(&text.replace("\r\n", "\n")) {
                        return Some((Ok(event), Some((bytes, buffer, start, scanned))));
                    }
                    continue;
                }
                if buffer.len() - start > limit {
                    return Some((Err(ClientError::EventTooLarge { limit }), None));
                }
                // A delimiter may start up to three bytes before the end.
                scanned = buffer.len().saturating_sub(3).max(start);
                // Compact once per chunk, not once per event.
                if start > 0 {
                    buffer.drain(..start);
                    scanned -= start;
                    start = 0;
                }
                match bytes.next().await? {
                    Ok(chunk) => buffer.extend_from_slice(&chunk),
                    Err(error) => {
                        return Some((Err(error.into()), Some((bytes, buffer, start, scanned))));
                    }
                }
            }
        },
    )
}

/// The end (exclusive) of the first complete block at or after `from`: a
/// blank line, written `\n\n` or `\r\n\r\n`, whichever comes first. One
/// pass, so a run of blank lines costs a step per byte, not per search.
fn block_end(buffer: &[u8], from: usize) -> Option<usize> {
    let mut at = from;
    while at < buffer.len() {
        match buffer[at] {
            b'\n' if buffer.get(at + 1) == Some(&b'\n') => return Some(at + 2),
            b'\r' if buffer[at..].starts_with(b"\r\n\r\n") => return Some(at + 4),
            _ => at += 1,
        }
    }
    None
}

pub(super) fn parse_block(block: &str) -> Option<Event> {
    let mut name = "message".to_owned();
    let mut data: Option<String> = None;
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            name = value.trim().to_owned();
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.strip_prefix(' ').unwrap_or(value);
            match &mut data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => data = Some(value.to_owned()),
            }
        }
    }
    data.map(|data| Event { name, data })
}

#[cfg(test)]
mod tests;
