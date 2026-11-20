//! An accepted session's life: authenticate on the control stream, hand every
//! further bidirectional stream to [`crate::subscription`], and end it with
//! 4401 / 4403 / 4500 when the credential expires or the principal is revoked.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cratestack_core::subscription_wire::SessionCharger;
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RevocationWatch, StreamEndNotice,
    StreamEndReason, SubscriptionDispatcher,
};
use tokio::sync::Semaphore;
use tokio::time::Instant;
use tracing::Instrument;

use crate::admission::{Connect, authenticate, init_headers, refuse};
use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame, close_code};
use crate::principal::session_key;
use crate::reader::{FrameReader, ReadFailure};
use crate::server::Shared;
use crate::session::ending::Ending;
use crate::subscription::{self, StreamCtx};
use crate::transport::Session;

/// When the context's `exp` claim passes, if it has one.
fn expiry_of(ctx: &CratestackContext) -> Option<Instant> {
    let exp = ctx.expires_at()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(Instant::now() + Duration::from_secs(exp.saturating_sub(now).max(0) as u64))
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Runs an accepted session to its end; returns the close code and reason.
pub(super) async fn established<A: AuthProvider, D: SubscriptionDispatcher>(
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
    // A principal revoked before this session connected is refused now.
    let revocations = shared.dispatcher.revocations();
    let mut revocation = match revocations.map(|registry| registry.watch_context(&ctx)) {
        Some(Ok(watch)) => Some(watch),
        Some(Err(end)) => {
            return refuse(
                &mut control_send,
                codec,
                &end.into_error(),
                "principal revoked",
            )
            .await;
        }
        None => None,
    };
    let principal = (shared.principal_key)(&ctx);
    let charger = SessionCharger::new(shared.rate_limit.clone(), principal.clone(), Some(remote));
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
    let ending = Ending::new();
    loop {
        // `control.next()` is dropped whenever a new stream wins the race.
        // That is safe only because its one await point, quinn's
        // `read_chunk`, is cancel-safe, and the frames a chunk completes are
        // queued before anything else awaits (`FrameReader::next`). Keep it
        // that way, or a control frame can be lost.
        let expiry = expiry_of(&ctx_rx.borrow());
        // Biased, revocation first: a revoke and another event ready in the
        // same poll must not let the other win and outlive it.
        tokio::select! {
            biased;
            end = RevocationWatch::ended_or_pending(&mut revocation) => {
                ending.end_streams(&end).await;
                return refuse(&mut control_send, codec, &end.clone().into_error(), end.reason.as_str()).await;
            }
            () = sleep_until(expiry) => {
                let end = StreamEndNotice::new(StreamEndReason::CredentialExpired);
                ending.end_streams(&end).await;
                return refuse(&mut control_send, codec, &end.into_error(), "credential expired").await;
            }
            stream = session.accept_bi() => {
                let Some((send, recv)) = stream else { return (0, "session ended") };
                let stream = StreamCtx {
                    dispatcher: shared.dispatcher.clone(),
                    codec,
                    max_frame_bytes: limits.max_frame_bytes,
                    ctx: ctx_rx.borrow().clone(),
                    permit: Arc::clone(&subscriptions).try_acquire_owned().ok(),
                    charger: charger.clone(),
                    ending: ending.handle(),
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
                        // Re-registered under the refreshed keys; streams already
                        // open keep the context they opened with.
                        if let Some(registry) = revocations {
                            // Register the new watch BEFORE looking at the old
                            // one: a revoke that lands between the two ends
                            // both and the check sees it, where checking
                            // first and registering after would let it end
                            // the old watch (then dropped) and miss the new.
                            let watch = match registry.watch_context(&refreshed) {
                                Ok(watch) => watch,
                                Err(end) => {
                                    return refuse(&mut control_send, codec, &end.into_error(), "principal revoked").await;
                                }
                            };
                            if let Some(end) = revocation.as_ref().and_then(RevocationWatch::try_ended) {
                                drop(watch);
                                ending.end_streams(&end).await;
                                return refuse(&mut control_send, codec, &end.clone().into_error(), end.reason.as_str()).await;
                            }
                            revocation = Some(watch);
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
