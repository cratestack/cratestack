//! One WebTransport session (design §6a.2, §6a.4): check the CONNECT,
//! authenticate on the control stream, then hand every further
//! bidirectional stream to [`crate::subscription`]. The session ends when its
//! credential expires or its principal is revoked (cratestack#1381).

use std::sync::Arc;

use http::StatusCode;
use tracing::Instrument;

use cratestack_core::{AuthProvider, SubscriptionDispatcher};

use crate::admission::Connect;
use crate::codec::WireCodec;
use crate::server::Shared;
use crate::transport::Incoming;

mod ending;
mod established;

pub(crate) use ending::Handle as StreamEnding;
use established::established;

pub(crate) async fn run<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: Arc<Shared<A, D>>,
    incoming: Incoming,
) {
    let remote = incoming.remote();
    let span = tracing::info_span!("cratestack_webtransport_session", %remote, close = tracing::field::Empty);
    async move {
        if incoming.path() != shared.path {
            return incoming.reject(StatusCode::NOT_FOUND).await;
        }
        let origin = incoming
            .headers()
            .get(http::header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        if !shared.origins.permits(origin, &incoming.authority()) {
            tracing::info!(target: "cratestack", origin, "WebTransport origin refused");
            return incoming.reject(StatusCode::FORBIDDEN).await;
        }
        let Ok((codec, negotiated)) = WireCodec::select(incoming.protocols(), incoming.query())
        else {
            return incoming.reject(StatusCode::NOT_ACCEPTABLE).await;
        };
        let connect = Connect {
            method: "CONNECT",
            path: incoming.path().to_owned(),
            query: incoming.query().map(str::to_owned),
            headers: incoming.headers().clone(),
        };
        let Ok(session) = incoming.accept(negotiated.then(|| codec.protocol())).await else {
            return;
        };
        let (code, reason) = established(&shared, &session, codec, &connect, remote.ip()).await;
        tracing::Span::current().record("close", code);
        session.close(code, reason).await;
    }
    .instrument(span)
    .await
}
