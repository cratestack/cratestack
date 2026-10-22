//! The axum surface: `POST /sync/{register,push,pull}`, `GET /sync/poke`.
//!
//! Authentication is the application's existing [`AuthProvider`] — the
//! same one its generated routes use — so sync adds no second identity
//! system. Bodies are capped at `max_body_bytes` before decoding.

mod stream;

use axum::Router;
use axum::body::{Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cratestack_core::{AuthProvider, CratestackContext, RequestContext};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{SyncError, SyncServer};
use crate::protocol::SyncErrorCode;

#[derive(Clone)]
pub(crate) struct HttpState<A> {
    pub(crate) server: SyncServer,
    pub(crate) auth: A,
}

/// Mount next to the generated router: `app.merge(sync_router(server, auth))`.
pub fn sync_router<A: AuthProvider>(server: SyncServer, auth: A) -> Router {
    Router::new()
        .route("/sync/register", post(register::<A>))
        .route("/sync/push", post(push::<A>))
        .route("/sync/pull", post(pull::<A>))
        .route("/sync/poke", get(stream::poke::<A>))
        .with_state(HttpState { server, auth })
}

async fn register<A: AuthProvider>(
    State(state): State<HttpState<A>>,
    request: Request,
) -> Response {
    let result = async {
        let (ctx, body) = state.read(request).await?;
        state.server.register(&ctx, body).await
    };
    respond(result.await)
}

async fn push<A: AuthProvider>(State(state): State<HttpState<A>>, request: Request) -> Response {
    let result = async {
        let (ctx, body) = state.read(request).await?;
        state.server.push(&ctx, body).await
    };
    respond(result.await)
}

async fn pull<A: AuthProvider>(State(state): State<HttpState<A>>, request: Request) -> Response {
    let result = async {
        let (ctx, body) = state.read(request).await?;
        state.server.pull(&ctx, body).await
    };
    respond(result.await)
}

impl<A: AuthProvider> HttpState<A> {
    /// Authenticate, then decode a bounded JSON body.
    async fn read<T: DeserializeOwned>(
        &self,
        request: Request,
    ) -> Result<(CratestackContext, T), SyncError> {
        let (parts, body) = request.into_parts();
        let limit = self.server.policy.max_body_bytes;
        let bytes = to_bytes(body, limit).await.map_err(|_| {
            SyncError::new(
                SyncErrorCode::MutationTooLarge,
                format!("request body exceeds {limit} bytes"),
            )
        })?;
        let ctx = self.authenticate(&parts, &bytes).await?;
        let decoded = serde_json::from_slice(&bytes)
            .map_err(|error| SyncError::new(SyncErrorCode::BadRequest, error.to_string()))?;
        Ok((ctx, decoded))
    }

    pub(crate) async fn authenticate(
        &self,
        parts: &Parts,
        body: &Bytes,
    ) -> Result<CratestackContext, SyncError> {
        let request = RequestContext {
            method: parts.method.as_str(),
            path: parts.uri.path(),
            query: parts.uri.query(),
            headers: &parts.headers,
            body,
            extensions: &parts.extensions,
        };
        self.auth.authenticate(&request).await.map_err(|error| {
            let error: cratestack_core::CratestackError = error.into();
            SyncError::new(SyncErrorCode::Unauthenticated, error.to_string())
        })
    }
}

pub(crate) fn respond<T: Serialize>(result: Result<T, SyncError>) -> Response {
    match result {
        Ok(body) => (StatusCode::OK, axum::Json(body)).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) fn error_response(error: SyncError) -> Response {
    let status =
        StatusCode::from_u16(error.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let retry_after = error.retry_after_secs;
    let mut response = (status, axum::Json(error)).into_response();
    if let Some(secs) = retry_after
        && let Ok(value) = secs.to_string().parse()
    {
        response.headers_mut().insert("retry-after", value);
    }
    response
}
