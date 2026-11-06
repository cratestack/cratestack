//! `GET /sync/poke?clientId=…` — a server-sent-events stream of
//! payload-free `poke` events, one per committed change in the caller's
//! scopes (and one on connect, so a reconnecting client always pulls).
//!
//! Browser `EventSource` cannot send an `Authorization` header; clients
//! read this stream with `fetch` instead, which can.

use std::convert::Infallible;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{Query, Request, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use cratestack_core::AuthProvider;
use futures_util::Stream;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::{HttpState, error_response};
use crate::protocol::SyncErrorCode;
use crate::server::clients::authorize;
use crate::server::poke::{ALL_SCOPES, StreamSlot};
use crate::server::{SyncError, SyncServer, internal};

/// The poke route, and the `path` the provider is given (see `REGISTER`).
pub(super) const POKE: &str = "/sync/poke";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PokeQuery {
    client_id: Uuid,
}

pub(crate) async fn poke<A: AuthProvider>(
    State(state): State<HttpState<A>>,
    Query(query): Query<PokeQuery>,
    request: Request,
) -> Response {
    let (parts, _) = request.into_parts();
    match open(&state, &parts, query.client_id).await {
        Ok(stream) => Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response(),
        Err(error) => error_response(error),
    }
}

async fn open<A: AuthProvider>(
    state: &HttpState<A>,
    parts: &axum::http::request::Parts,
    client_id: Uuid,
) -> Result<impl Stream<Item = Result<Event, Infallible>> + use<A>, SyncError> {
    let server: &SyncServer = &state.server;
    let ctx = state.authenticate(parts, POKE, &Bytes::new()).await?;
    let principal = server.principal(&ctx)?;
    server.admit("poke", &principal.id).await?;
    let mut conn = server.pool.acquire().await.map_err(internal)?;
    authorize(&mut conn, &principal, client_id, false).await?;
    drop(conn);

    let slot: StreamSlot = server
        .pokes
        .open_stream(&principal.id, server.policy.max_poke_streams_per_principal)
        .ok_or_else(|| {
            SyncError::new(
                SyncErrorCode::TooManyStreams,
                "too many open poke streams for this principal",
            )
        })?;
    let mut receiver = server.pokes.subscribe();
    let scopes = principal.scopes;

    Ok(async_stream::stream! {
        let _slot = slot;
        yield Ok(Event::default().event("poke").data(""));
        loop {
            let poke = match receiver.recv().await {
                Ok(scope) => scope == ALL_SCOPES || scopes.contains(&scope),
                // Fell behind: pokes are idempotent, so one covers them all.
                Err(RecvError::Lagged(_)) => true,
                Err(RecvError::Closed) => break,
            };
            if poke {
                yield Ok(Event::default().event("poke").data(""));
            }
        }
    })
}
