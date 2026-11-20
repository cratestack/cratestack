//! The established half of a socket's life (design §6.3–6.6): the select
//! loop that serves `Request`s, `Cancel`s and refreshes, pings, and ends the
//! socket when its credential expires or its principal is revoked
//! (cratestack#1381).

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use cratestack_core::subscription_wire::{
    ClientMessage, SessionCharger, authenticate_session, close_code,
};
use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RevocationWatch, StreamEndNotice,
    StreamEndReason, SubscriptionDispatcher,
};
use cratestack_core::{SubscriptionOpen, subscription_wire::ClientFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;
use tracing::Instrument;

use super::io::{drain_close, expiry_of, send, sleep_until};
use crate::close::{Sink, close, end_session, refuse, ws_close};
use crate::codec::WireCodec;
use crate::server::Shared;
use crate::subscription::{self, Outbound};
use cratestack_core::subscription_wire::{Handshake, ServerFrame};

pub(super) async fn established<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: &Shared<A, D>,
    mut sink: Sink,
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    codec: WireCodec,
    handshake: &Handshake,
    mut ctx: CratestackContext,
    mut revocation: Option<RevocationWatch>,
    charger: SessionCharger,
) {
    let revocations = shared.dispatcher.revocations();
    let limits = &shared.config.limits;
    let (out, mut outbound) = mpsc::channel(limits.outbound_queue);
    let overflow = Arc::new(Notify::new());
    let mut open: HashMap<u64, tokio::task::AbortHandle> = HashMap::new();
    let mut ping = tokio::time::interval(limits.ping_interval);
    let mut last_pong = Instant::now();
    loop {
        let expiry = expiry_of(&ctx);
        // Biased, revocation first: a revoke and another event ready in the
        // same poll must not let the other win and outlive it. Rare events
        // (expiry, overflow, ping) precede the frame arms; a client flooding
        // `Request`s is bounded by the per-socket limit and the rate limiter.
        tokio::select! {
            biased;
            end = RevocationWatch::ended_or_pending(&mut revocation) => {
                let (open, error) = (open.keys().copied().collect(), end.clone().into_error());
                end_session(&mut sink, codec, open, &error, end.reason.as_str()).await;
                break;
            }
            () = sleep_until(expiry) => {
                let error = StreamEndNotice::new(StreamEndReason::CredentialExpired).into_error();
                let open = open.keys().copied().collect();
                end_session(&mut sink, codec, open, &error, "credential expired").await;
                break;
            }
            () = overflow.notified() => {
                close(&mut sink, ws_close::TRY_AGAIN_LATER, "client too slow").await;
                break;
            }
            _ = ping.tick() => {
                if last_pong.elapsed() > limits.pong_timeout {
                    close(&mut sink, ws_close::GOING_AWAY, "no pong").await;
                    break;
                }
                if sink.send(Message::Ping(Default::default())).await.is_err() { break; }
            }
            message = stream.next() => {
                let message = match message {
                    Some(Ok(Message::Pong(_))) => { last_pong = Instant::now(); continue; }
                    Some(Ok(Message::Ping(_))) => continue,
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(message)) => message,
                };
                match codec.decode(&message).and_then(ClientFrame::classify) {
                    Ok(ClientMessage::Request { id, op, input }) => {
                        let refusal = if id == 0 || open.contains_key(&id) {
                            Some(CratestackError::BadRequest(format!("subscription id {id} is in use")))
                        } else if open.len() >= limits.subscriptions_per_socket {
                            Some(CratestackError::TooManyRequests("too many subscriptions on this socket".to_owned()))
                        } else { None };
                        if let Some(error) = refusal {
                            if send(&mut sink, codec, &ServerFrame::err(id, &error)).await.is_err() { break; }
                            continue;
                        }
                        let request = SubscriptionOpen { op_id: op, input: input.into(), ctx: ctx.clone() };
                        let task = subscription::run(shared.dispatcher.clone(), request, id, out.clone(), overflow.clone(), charger.clone(), codec);
                        open.insert(id, tokio::spawn(task.in_current_span()).abort_handle());
                    }
                    Ok(ClientMessage::Cancel { id }) => {
                        if let Some(task) = open.remove(&id) {
                            task.abort();
                            if send(&mut sink, codec, &ServerFrame::end(id)).await.is_err() { break; }
                        }
                    }
                    Ok(ClientMessage::Init(init)) => {
                        // Re-authentication: must name the same principal (§6.3).
                        let refreshed = match authenticate_session(&shared.auth, handshake, &init.headers).await {
                            Ok(refreshed) => refreshed,
                            Err(error) => { refuse(&mut sink, codec, &error, "re-authentication failed").await; break; }
                        };
                        if (shared.config.principal_key)(&refreshed).as_deref() != charger.principal() {
                            let error = CratestackError::Forbidden("re-authentication named another principal".to_owned());
                            refuse(&mut sink, codec, &error, "principal changed").await;
                            break;
                        }
                        // Re-registered under the refreshed keys; a key that
                        // was revoked meanwhile refuses the refresh. Streams
                        // already open keep the context they opened with.
                        if let Some(revocations) = revocations {
                            // Register the new watch BEFORE looking at the old
                            // one: a revoke that lands between the two ends
                            // both and the check sees it, where checking
                            // first and registering after would let it end
                            // the old watch (then dropped) and miss the new.
                            let watch = match revocations.watch_context(&refreshed) {
                                Ok(watch) => watch,
                                Err(end) => {
                                    refuse(&mut sink, codec, &end.into_error(), "principal revoked").await;
                                    break;
                                }
                            };
                            if let Some(end) = revocation.as_ref().and_then(RevocationWatch::try_ended) {
                                drop(watch);
                                refuse(&mut sink, codec, &end.clone().into_error(), end.reason.as_str()).await;
                                break;
                            }
                            revocation = Some(watch);
                        }
                        ctx = refreshed;
                    }
                    Err(error) => {
                        tracing::info!(target: "cratestack", %error, "WebSocket protocol error");
                        close(&mut sink, close_code::PROTOCOL as u16, "bad frame").await;
                        break;
                    }
                }
            }
            outbound = outbound.recv() => match outbound {
                Some(Outbound::Frame(frame)) => if send(&mut sink, codec, &frame).await.is_err() { break },
                Some(Outbound::Done(id)) => { open.remove(&id); }
                None => break,
            },
        }
    }
    for task in open.into_values() {
        task.abort();
    }
    drain_close(&mut stream).await;
}
