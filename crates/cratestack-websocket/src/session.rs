//! One socket (design §6.3–6.6): `Init` within the timeout, authenticated
//! like the HTTP routes; then `Request`s open subscriptions, `Cancel` ends
//! one, a further `Init` refreshes the credentials (same principal only),
//! and the server pings. The socket closes with 4401 when the context's
//! `exp` passes without a refresh, or when its principal is revoked.

use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::ws::WebSocket;
use cratestack_core::subscription_wire::{
    Handshake, ServerFrame, SessionCharger, authenticate_session, close_code, init_headers,
    session_key,
};
use cratestack_core::{AuthProvider, SubscriptionDispatcher};
use futures_util::StreamExt;
use tracing::Instrument;

use crate::close::{close, refuse};
use crate::codec::WireCodec;
use crate::server::Shared;

mod established;
mod io;

use established::established;
use io::{next_frame, send};

pub(crate) async fn run<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: Arc<Shared<A, D>>,
    socket: WebSocket,
    codec: WireCodec,
    handshake: Handshake,
    remote: IpAddr,
    peer: Option<IpAddr>,
) {
    let span =
        tracing::info_span!("cratestack_websocket_session", %remote, close = tracing::field::Empty);
    async move {
        let (mut sink, mut stream) = socket.split();
        let limits = &shared.config.limits;
        let first = tokio::time::timeout(limits.init_timeout, next_frame(&mut stream, codec)).await;
        let Some(init) = first
            .ok()
            .flatten()
            .and_then(|frame| frame.ok())
            .and_then(init_headers)
        else {
            return close(
                &mut sink,
                close_code::UNAUTHENTICATED as u16,
                "expected Init first",
            )
            .await;
        };
        let ctx = match authenticate_session(&shared.auth, &handshake, &init).await {
            Ok(ctx) => ctx,
            Err(error) => return refuse(&mut sink, codec, &error, "authentication failed").await,
        };
        // A principal revoked before this socket connected is refused now.
        let revocation = match shared.dispatcher.revocations() {
            Some(revocations) => match revocations.watch_context(&ctx) {
                Ok(watch) => Some(watch),
                Err(end) => {
                    return refuse(&mut sink, codec, &end.into_error(), "principal revoked").await;
                }
            },
            None => None,
        };
        let principal = (shared.config.principal_key)(&ctx);
        let key = session_key(principal.as_deref(), remote);
        let Some(_slot) = shared.registry.admit(key, limits.sockets_per_principal) else {
            return close(
                &mut sink,
                close_code::TOO_MANY_SESSIONS as u16,
                "too many sockets",
            )
            .await;
        };
        if send(&mut sink, codec, &ServerFrame::ready(0))
            .await
            .is_err()
        {
            return;
        }
        let charger = SessionCharger::new(shared.config.rate_limit.clone(), principal, peer);
        established(
            &shared, sink, stream, codec, &handshake, ctx, revocation, charger,
        )
        .await;
    }
    .instrument(span)
    .await
}
