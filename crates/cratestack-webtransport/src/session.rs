//! One WebTransport session (design §6a.2, §6a.4): check the CONNECT,
//! authenticate on the control stream, then hand every further
//! bidirectional stream to [`crate::subscription`].

use std::collections::BTreeMap;
use std::sync::Arc;

use cratestack_core::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, SubscriptionDispatcher,
};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use tokio::sync::Semaphore;
use tracing::Instrument;

use crate::codec::WireCodec;
use crate::frame::{ClientFrame, ClientMessage, ServerFrame, close_code};
use crate::principal::session_key;
use crate::reader::{FrameReader, ReadFailure};
use crate::server::Shared;
use crate::subscription::{self, StreamCtx};
use crate::transport::{Incoming, Session};

/// What the CONNECT carried that authentication needs later.
struct Connect {
    path: String,
    query: Option<String>,
    headers: HeaderMap,
}

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
    let Ok(ctx) = authenticate(&shared.auth, connect, &init).await else {
        return (close_code::UNAUTHENTICATED, "authentication failed");
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
                        let Ok(refreshed) = authenticate(&shared.auth, connect, &init.headers).await else {
                            return (close_code::UNAUTHENTICATED, "re-authentication failed");
                        };
                        if (shared.principal_key)(&refreshed) != principal {
                            return (close_code::PRINCIPAL_CHANGED, "re-authentication named another principal");
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

/// The first control frame must be `Init`.
fn init_headers(frame: ClientFrame) -> Option<BTreeMap<String, String>> {
    match frame.classify() {
        Ok(ClientMessage::Init(init)) => Some(init.headers),
        _ => None,
    }
}

/// Runs the app's `AuthProvider` over the CONNECT request. `Init` headers,
/// when present, stand in for the CONNECT headers: that is how a browser,
/// which cannot set headers on `new WebTransport(url)`, presents
/// credentials. A native client sends them on CONNECT and an empty `Init`.
async fn authenticate<A: AuthProvider>(
    auth: &A,
    connect: &Connect,
    init_headers: &BTreeMap<String, String>,
) -> Result<CratestackContext, CratestackError> {
    let from_init;
    let headers = if init_headers.is_empty() {
        &connect.headers
    } else {
        let mut headers = HeaderMap::new();
        for (name, value) in init_headers {
            let name =
                HeaderName::from_bytes(name.to_ascii_lowercase().as_bytes()).map_err(|_| {
                    CratestackError::BadRequest(format!("invalid Init header name `{name}`"))
                })?;
            let value = HeaderValue::from_str(value).map_err(|_| {
                CratestackError::BadRequest(format!("invalid Init header value for `{name}`"))
            })?;
            headers.append(name, value);
        }
        from_init = headers;
        &from_init
    };
    let extensions = http::Extensions::new();
    let request = RequestContext {
        method: "CONNECT",
        path: &connect.path,
        query: connect.query.as_deref(),
        headers,
        body: &[],
        extensions: &extensions,
    };
    auth.authenticate(&request).await.map_err(Into::into)
}
