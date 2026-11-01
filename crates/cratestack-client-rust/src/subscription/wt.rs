//! A subscription over WebTransport (`docs/design/subscriptions-websocket.md`
//! §6a, feature `webtransport`, native targets): one session per
//! subscription, CBOR frames, through `cratestack-webtransport`'s client.
//! `Init` carries the request authorizer's headers for `GET <session path>`;
//! the call resolves at the subscription's `Ready`. Dropping the
//! subscription stops its stream, which is the cancel.

use std::collections::BTreeMap;

use cratestack_webtransport::{ClientFailure, WebTransportClient};
use reqwest::Method;
use serde::de::DeserializeOwned;

use super::failure::Failure;
use super::frames::{item, rpc_error_to_client};
use super::{ClientSubscription, SubscriptionTarget};
use crate::client::CratestackClient;
use crate::codec::HttpClientCodec;
use crate::config::WebTransportEndpoint;
use crate::error::ClientError;

fn failure(failure: ClientFailure) -> Failure {
    match failure {
        ClientFailure::Refused(body) => Failure::Refused(rpc_error_to_client(body)),
        ClientFailure::Unreachable(reason) => Failure::Unreachable(ClientError::InvalidResponse(
            format!("WebTransport: {reason}"),
        )),
    }
}

/// The session, authenticated: `Ready{0}` has arrived.
pub(super) async fn connect<C: HttpClientCodec>(
    client: &CratestackClient<C>,
    endpoint: &WebTransportEndpoint,
) -> Result<WebTransportClient, Failure> {
    let headers = client
        .build_header_map(&Method::GET, endpoint.url.path(), None, None, &[], "*/*")
        .await
        .map_err(Failure::Refused)?;
    let headers: BTreeMap<String, String> = headers
        .iter()
        .filter(|(name, _)| *name != reqwest::header::ACCEPT)
        .filter_map(|(name, value)| {
            Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
        })
        .collect();
    WebTransportClient::connect(&endpoint.url, &endpoint.trust, headers)
        .await
        .map_err(failure)
}

/// `target` on `session`, resolving at its `Ready`.
pub(super) async fn open<T: DeserializeOwned + Send + 'static>(
    session: WebTransportClient,
    target: &SubscriptionTarget,
) -> Result<ClientSubscription<T>, Failure> {
    let mut subscription = session
        .subscribe(&target.op_id, target.input.clone())
        .await
        .map_err(failure)?;
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        // The session lives as long as the pump: dropping it ends the session.
        let _session = session;
        loop {
            let next = tokio::select! {
                next = subscription.next() => next,
                () = tx.closed() => return,
            };
            let Some(next) = next else { return };
            let next = next
                .map_err(|error| failure(error).into_error())
                .and_then(item::<T>);
            let last = next.is_err();
            if tx.send(next).await.is_err() || last {
                return;
            }
        }
    });
    Ok(rx)
}
