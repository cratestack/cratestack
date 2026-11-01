//! One socket (design §6.3–6.6): `Init` within the timeout, authenticated
//! like the HTTP routes; then `Request`s open subscriptions, `Cancel` ends
//! one, a further `Init` refreshes the credentials (same principal only),
//! and the server pings. The socket closes with 4401 when the context's
//! `exp` passes without a refresh.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use cratestack_core::subscription_wire::{
    ClientMessage, Handshake, ServerFrame, authenticate_session, close_code, init_headers,
    session_key,
};
use cratestack_core::{AuthProvider, CratestackContext, CratestackError, SubscriptionDispatcher};
use cratestack_core::{SubscriptionOpen, subscription_wire::ClientFrame};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;
use tracing::Instrument;

use crate::close::{Sink, close, refuse, ws_close};
use crate::codec::WireCodec;
use crate::server::Shared;
use crate::subscription::{self, Outbound};

pub(crate) async fn run<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: Arc<Shared<A, D>>,
    socket: WebSocket,
    codec: WireCodec,
    handshake: Handshake,
    remote: IpAddr,
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
        established(&shared, sink, stream, codec, &handshake, ctx, principal).await;
    }
    .instrument(span)
    .await
}

async fn established<A: AuthProvider, D: SubscriptionDispatcher>(
    shared: &Shared<A, D>,
    mut sink: Sink,
    mut stream: futures_util::stream::SplitStream<WebSocket>,
    codec: WireCodec,
    handshake: &Handshake,
    mut ctx: CratestackContext,
    principal: Option<String>,
) {
    let limits = &shared.config.limits;
    let (out, mut outbound) = mpsc::channel(limits.outbound_queue);
    let overflow = Arc::new(Notify::new());
    let mut open: HashMap<u64, tokio::task::AbortHandle> = HashMap::new();
    let mut ping = tokio::time::interval(limits.ping_interval);
    let mut last_pong = Instant::now();
    loop {
        let expiry = expiry_of(&ctx);
        tokio::select! {
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
                        let request = SubscriptionOpen { op_id: op, input, ctx: ctx.clone() };
                        let task = subscription::run(shared.dispatcher.clone(), request, id, out.clone(), overflow.clone());
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
                        if (shared.config.principal_key)(&refreshed) != principal {
                            let error = CratestackError::Forbidden("re-authentication named another principal".to_owned());
                            refuse(&mut sink, codec, &error, "principal changed").await;
                            break;
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
            () = sleep_until(expiry) => {
                let error = CratestackError::Unauthorized("credential expired".to_owned());
                refuse(&mut sink, codec, &error, "credential expired").await;
                break;
            }
        }
    }
    for task in open.into_values() {
        task.abort();
    }
}

async fn next_frame(
    stream: &mut futures_util::stream::SplitStream<WebSocket>,
    codec: WireCodec,
) -> Option<Result<ClientFrame, CratestackError>> {
    loop {
        match stream.next().await? {
            Ok(Message::Ping(_) | Message::Pong(_)) => continue,
            Ok(message @ (Message::Text(_) | Message::Binary(_))) => {
                return Some(codec.decode(&message));
            }
            Ok(Message::Close(_)) | Err(_) => return None,
        }
    }
}

async fn send(sink: &mut Sink, codec: WireCodec, frame: &ServerFrame) -> Result<(), ()> {
    let message = codec.encode(frame).map_err(|error| {
        tracing::warn!(target: "cratestack", %error, "WebSocket frame failed to encode");
    })?;
    sink.send(message).await.map_err(|_| ())
}

/// When the context's `exp` claim passes, if it has one.
fn expiry_of(ctx: &CratestackContext) -> Option<Instant> {
    let exp = ctx.expires_at()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    Some(Instant::now() + std::time::Duration::from_secs(exp.saturating_sub(now).max(0) as u64))
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}
