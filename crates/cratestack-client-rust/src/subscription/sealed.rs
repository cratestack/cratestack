//! A subscription opened over the signed transport (ADR 0006,
//! cratestack#1378): the open is sealed (`client/sealed_stream.rs`), and the
//! unsigned stream that answers is read like a plain one.
//!
//! What a client with an envelope can open is narrow, and everything else is
//! [`EnvelopeError::StreamsUnsupported`], refused before anything is sent:
//! an unmarked subscription, a WebSocket or WebTransport pin (a socket has
//! no sealed open), a REST route on `wasm32` (its open is a `GET` with a
//! body, which `fetch` refuses; use `transport rpc`). `Auto` picks cbor-seq,
//! the one binding that keeps the CBOR shapes, and never falls back to a
//! socket.

use serde::de::DeserializeOwned;

use super::{ClientSubscription, FramesMode, SubscriptionTarget, SubscriptionTransport};
use super::{cbor_seq, sse};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::envelope_error::EnvelopeError;
use crate::error::ClientError;

/// Which HTTP reader a sealed open uses: `true` for SSE, `false` for
/// cbor-seq. Everything this client cannot open signed is
/// [`EnvelopeError::StreamsUnsupported`], decided before anything is sent:
/// `wasm` says the target is `wasm32`.
pub(super) fn choose(
    transport: SubscriptionTransport,
    frames: Option<FramesMode>,
    seal_route: &str,
    wasm: bool,
) -> Result<bool, EnvelopeError> {
    let sse = match transport {
        SubscriptionTransport::Sse => true,
        SubscriptionTransport::CborSeq | SubscriptionTransport::Auto => false,
        SubscriptionTransport::WebSocket | SubscriptionTransport::WebTransport => {
            return Err(EnvelopeError::StreamsUnsupported);
        }
    };
    let rest = seal_route.starts_with('/');
    if frames != Some(FramesMode::Unsigned) || (rest && wasm) {
        return Err(EnvelopeError::StreamsUnsupported);
    }
    Ok(sse)
}

pub(super) async fn open<C: HttpClientCodec, T: DeserializeOwned + Send + 'static>(
    client: &CratestackClient<C>,
    target: &SubscriptionTarget,
) -> Result<ClientSubscription<T>, ClientError> {
    let sse = choose(
        client.config.subscription_transport,
        target.frames,
        &target.seal_route,
        cfg!(target_arch = "wasm32"),
    )?;
    match sse {
        true => {
            let response = client.open_sealed_stream(target, sse::ACCEPT).await?;
            sse::read(client, response).await
        }
        false => {
            let response = client.open_sealed_stream(target, cbor_seq::ACCEPT).await?;
            cbor_seq::read(client, response).await
        }
    }
}

#[cfg(test)]
mod tests;
