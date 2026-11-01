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
    let mut events = Box::pin(events(response.bytes_stream()));
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
pub(super) fn events<S>(bytes: S) -> impl Stream<Item = Result<Event, ClientError>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    futures_util::stream::unfold(
        (bytes, Vec::<u8>::new()),
        |(mut bytes, mut buffer)| async move {
            loop {
                if let Some(end) = block_end(&buffer) {
                    let block: Vec<u8> = buffer.drain(..end).collect();
                    let Ok(text) = std::str::from_utf8(&block) else {
                        let error =
                            ClientError::InvalidResponse("SSE body is not UTF-8".to_owned());
                        return Some((Err(error), (bytes, buffer)));
                    };
                    if let Some(event) = parse_block(&text.replace("\r\n", "\n")) {
                        return Some((Ok(event), (bytes, buffer)));
                    }
                    continue;
                }
                match bytes.next().await? {
                    Ok(chunk) => buffer.extend_from_slice(&chunk),
                    Err(error) => return Some((Err(error.into()), (bytes, buffer))),
                }
            }
        },
    )
}

/// The end (exclusive) of the first complete block: a blank line, written
/// `\n\n` or `\r\n\r\n`, whichever comes first.
fn block_end(buffer: &[u8]) -> Option<usize> {
    let find = |needle: &[u8]| {
        buffer
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| at + needle.len())
    };
    match (find(b"\n\n"), find(b"\r\n\r\n")) {
        (Some(lf), Some(crlf)) => Some(lf.min(crlf)),
        (lf, crlf) => lf.or(crlf),
    }
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
