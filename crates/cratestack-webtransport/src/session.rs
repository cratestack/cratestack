//! One WebTransport session (design §6a.2, §6a.4): check the CONNECT,
//! authenticate on the control stream, then hand every further
//! bidirectional stream to [`crate::subscription`].

use std::sync::Arc;

use cratestack_core::{AuthProvider, CratestackError, SubscriptionDispatcher};
use http::StatusCode;
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::admission::{Connect, authenticate, init_headers, refuse};
use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame, close_code};
use crate::principal::session_key;
use crate::reader::{FrameReader, ReadFailure};
use crate::server::Shared;
use crate::subscription::{self, StreamCtx};
use crate::transport::{Incoming, Session};

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

/// Runs an accepted session to its end; returns the close code and reason.
async fn established<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: &Shared<A, D>,
    session: &Session,
    codec: WireCodec,
    connect: &Connect,
    remote: std::net::IpAddr,
) -> (u32, &'static str) {
    let limits = &shared.limits;
    let first = tokio::time::timeout(limits.init_timeout, async {
        let (send, recv) = session.accept_bi().await?;
        let mut control = FrameReader::new(recv, codec, limits.max_frame_bytes);
        let init = control.next::<ClientFrame>().await.ok()?;
        Some((init_headers(init), control, send))
    })
    .await;
    let Ok(Some((Some(init), mut control, mut control_send))) = first else {
        return (
            close_code::UNAUTHENTICATED,
            "expected Init on the first stream",
        );
    };
    let ctx = match authenticate(&shared.auth, connect, &init).await {
        Ok(ctx) => ctx,
        Err(error) => {
            return refuse(&mut control_send, codec, &error, "authentication failed").await;
        }
    };
    let principal = (shared.principal_key)(&ctx);
    let key = session_key(principal.as_deref(), remote);
    let Some(_slot) = shared.registry.admit(key, limits.sessions_per_principal) else {
        return (
            close_code::TOO_MANY_SESSIONS,
            "too many sessions for this principal",
        );
    };
    let Ok(ready) = codec.encode(&ServerFrame::ready(0)) else {
        return (close_code::PROTOCOL, "encode failure");
    };
    if control_send.write_all(&ready).await.is_err() {
        return (0, "client went away");
    }

    let (ctx_tx, ctx_rx) = tokio::sync::watch::channel(ctx);
    let subscriptions = Arc::new(Semaphore::new(limits.subscriptions_per_session));
    loop {
        // `control.next()` is dropped whenever a new stream wins the race.
        // That is safe only because its one await point, quinn's
        // `read_chunk`, is cancel-safe, and the frames a chunk completes are
        // queued before anything else awaits (`FrameReader::next`). Keep it
        // that way, or a control frame can be lost.
        tokio::select! {
            stream = session.accept_bi() => {
                let Some((send, recv)) = stream else { return (0, "session ended") };
                let stream = StreamCtx {
                    dispatcher: shared.dispatcher.clone(),
                    codec,
                    max_frame_bytes: limits.max_frame_bytes,
                    ctx: ctx_rx.borrow().clone(),
                    permit: Arc::clone(&subscriptions).try_acquire_owned().ok(),
                };
                tokio::spawn(subscription::run(stream, send, recv).in_current_span());
            }
            frame = control.next::<ClientFrame>() => {
                match frame.map(ClientFrame::classify) {
                    Ok(Ok(ClientMessage::Init(init))) => {
                        // Re-authentication: must name the same principal (§6.3).
                        let refreshed = match authenticate(&shared.auth, connect, &init.headers).await {
                            Ok(refreshed) => refreshed,
                            Err(error) => {
                                return refuse(&mut control_send, codec, &error, "re-authentication failed").await;
                            }
                        };
                        if (shared.principal_key)(&refreshed) != principal {
                            let error = CratestackError::Forbidden(
                                "re-authentication named another principal".to_owned(),
                            );
                            return refuse(&mut control_send, codec, &error, "principal changed").await;
                        }
                        ctx_tx.send_replace(refreshed);
                    }
                    Ok(_) => return (close_code::PROTOCOL, "only Init is allowed on the control stream"),
                    Err(ReadFailure::Ended | ReadFailure::Gone) => return (0, "control stream closed"),
                    Err(ReadFailure::Protocol(error)) => {
                        tracing::info!(target: "cratestack", %error, "WebTransport control stream protocol error");
                        return (close_code::PROTOCOL, "bad frame on the control stream");
                    }
                }
            }
        }
    }
}
