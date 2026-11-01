//! The upgrade route (design §6.1, §6.3): `Origin`, then the subprotocol,
//! then the socket.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::Router;
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, State};
use axum::response::{IntoResponse, Response};
use cratestack_core::subscription_wire::{Handshake, SessionRegistry};
use cratestack_core::{AuthProvider, SubscriptionDispatcher};
use http::{Extensions, HeaderMap, StatusCode, Uri, header};

use crate::codec::WireCodec;
use crate::config::WebSocketConfig;

pub(crate) struct Shared<A, D> {
    pub(crate) auth: A,
    pub(crate) dispatcher: D,
    pub(crate) config: WebSocketConfig,
    pub(crate) registry: Arc<SessionRegistry>,
}

/// A router serving `dispatcher`'s subscriptions over WebSocket at the
/// configured path (`/rpc/ws` unless set), authenticating with `auth`.
/// Merge it into the application's router; under `into_make_service_with_connect_info`
/// anonymous sockets are counted per remote IP.
pub fn router<A, D>(config: WebSocketConfig, auth: A, dispatcher: D) -> Router
where
    A: AuthProvider,
    D: SubscriptionDispatcher,
{
    let config = config.default_path("/rpc/ws");
    let path = config.path.clone().unwrap_or_default();
    let shared = Arc::new(Shared {
        auth,
        dispatcher,
        config,
        registry: Arc::default(),
    });
    Router::new()
        .route(&path, axum::routing::get(upgrade::<A, D>))
        .with_state(shared)
}

async fn upgrade<A: AuthProvider, D: SubscriptionDispatcher>(
    State(shared): State<Arc<Shared<A, D>>>,
    ws: WebSocketUpgrade,
    headers: HeaderMap,
    uri: Uri,
    extensions: Extensions,
) -> Response {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let authority = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !shared.config.origins.permits_any_scheme(origin, authority) {
        tracing::info!(target: "cratestack", origin, "WebSocket origin refused");
        return StatusCode::FORBIDDEN.into_response();
    }
    let offered = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok());
    let Ok((codec, negotiated)) = WireCodec::select(offered, uri.query()) else {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    };
    let remote = extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|info| info.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let handshake = Handshake {
        method: "GET",
        path: uri.path().to_owned(),
        query: uri.query().map(str::to_owned),
        headers,
    };
    let limit = shared.config.limits.max_frame_bytes;
    let ws = ws.max_message_size(limit).max_frame_size(limit);
    let ws = if negotiated {
        ws.protocols([codec.protocol()])
    } else {
        ws
    };
    ws.on_upgrade(move |socket| crate::session::run(shared, socket, codec, handshake, remote))
}
