//! A chat that is live over every binding (slice S6,
//! `docs/design/subscriptions-websocket.md` §11).
//!
//! - **Messages** are a model feed: `model.Message.subscribe`, each event
//!   passing the model's read policy.
//! - **Typing indicators** are a topic, `RoomTyping`, behind the `typing`
//!   subscription procedure; `sendTyping` publishes to it.
//!
//! [`router`] serves both over SSE (`/rpc/subscribe/...`) and WebSocket
//! (`/rpc/ws`), next to the unary calls; [`webtransport`] serves them over
//! HTTP/3 on a second, UDP listener. The generated clients' `"auto"` picks
//! WebTransport, then the WebSocket, then SSE.

use std::net::SocketAddr;

use cratestack::websocket::WebSocketConfig;
use cratestack::webtransport::{
    CertificateDer, OriginPolicy, PrivateKeyDer, WebTransportConfig, WebTransportServer,
};
use cratestack::{
    AuthProvider, CratestackContext, CratestackError, RequestContext, SubscriptionStream, Value,
    include_server_schema,
};

include_server_schema!("schema.cstack", db = Postgres);

pub use cratestack_schema::{Cratestack, Message, Room, Typing};

/// The one table; the event outbox creates itself.
pub const SCHEMA_SQL: &str = "CREATE TABLE IF NOT EXISTS messages (
    id BIGINT PRIMARY KEY,
    room_id TEXT NOT NULL,
    author_id BIGINT NOT NULL,
    body TEXT NOT NULL
)";

/// `authorization: Bearer <id>` signs in as user `<id>`. A stand-in for a
/// real `AuthProvider` (a JWT, a session): every binding calls the same one,
/// over the request's headers or, from a browser, over the socket's `Init`.
#[derive(Clone)]
pub struct BearerId;

impl AuthProvider for BearerId {
    type Error = CratestackError;

    fn authenticate(
        &self,
        request: &RequestContext<'_>,
    ) -> impl core::future::Future<Output = Result<CratestackContext, Self::Error>> + Send {
        let id = request
            .headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|value| value.parse::<i64>().ok());
        core::future::ready(Ok(match id {
            Some(id) => CratestackContext::authenticated([("id".to_owned(), Value::Int(id))]),
            None => CratestackContext::anonymous(),
        }))
    }
}

/// The procedures: a topic subscription and the publish beside it.
#[derive(Clone)]
pub struct Chat;

impl cratestack_schema::procedures::ProcedureRegistry for Chat {
    async fn typing(
        &self,
        db: &Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::typing::Args,
        _authorized: cratestack_schema::procedures::typing::Authorized,
    ) -> Result<SubscriptionStream<Typing>, CratestackError> {
        db.topics().room_typing(&args.args.roomId).subscribe().await
    }

    async fn send_typing(
        &self,
        db: &Cratestack,
        _ctx: &CratestackContext,
        args: cratestack_schema::procedures::send_typing::Args,
        _authorized: cratestack_schema::procedures::send_typing::Authorized,
    ) -> Result<Typing, CratestackError> {
        db.topics()
            .room_typing(&args.args.roomId)
            .publish(&args.args)
            .await?;
        Ok(args.args)
    }
}

/// Unary calls, SSE subscriptions and the WebSocket, JSON on the wire so a
/// browser page can call it with `fetch`.
pub fn router(db: Cratestack) -> cratestack::axum::Router {
    cratestack_schema::axum::rpc_router(
        db.clone(),
        Chat,
        (),
        cratestack_codec_json::JsonCodec,
        BearerId,
        cratestack::DEFAULT_BODY_LIMIT_BYTES,
    )
    .merge(
        db.subscriptions(Chat)
            .websocket(WebSocketConfig::new(), BearerId),
    )
}

/// The bound WebTransport listener: `local_addr()`, then `serve()`.
pub type ChatWebTransport =
    WebTransportServer<BearerId, cratestack_schema::subscriptions::SubscriptionDispatch<Chat>>;

/// The WebTransport listener on `bind` (UDP). Browser pages are served
/// from another origin (the HTTP listener), so it is allowed explicitly.
pub fn webtransport(
    db: &Cratestack,
    bind: SocketAddr,
    certificate_chain: Vec<CertificateDer<'static>>,
    private_key: PrivateKeyDer<'static>,
    page_origin: &str,
) -> Result<ChatWebTransport, CratestackError> {
    let config = WebTransportConfig::new(bind, certificate_chain, private_key)
        .origins(OriginPolicy::Allow(vec![page_origin.to_owned()]));
    db.subscriptions(Chat).webtransport(config, BearerId)
}
