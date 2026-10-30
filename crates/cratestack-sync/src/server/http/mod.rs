//! The axum surface: `POST /sync/{register,push,pull}`, `GET /sync/poke`.
//!
//! Authentication is the application's existing [`AuthProvider`] — the
//! same one its generated routes use — so sync adds no second identity
//! system. Bodies are capped at `max_body_bytes` before decoding.
//!
//! A provider error reads as it does on REST (cratestack#1243): its public
//! message, never an operator detail; 403 `forbidden` for a caller the
//! provider knows and refuses, 500 `internal` for a provider that could not
//! decide, and 401 `unauthenticated` for the rest. An
//! [`AuthRefusal`](cratestack_core::AuthRefusal) also keeps its `details`
//! and members in the body and its challenges as `WWW-Authenticate`.

mod stream;

use axum::Router;
use axum::body::{Bytes, to_bytes};
use axum::extract::{Request, State};
use axum::http::HeaderValue;
use axum::http::StatusCode;
use axum::http::header::WWW_AUTHENTICATE;
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use cratestack_core::{AuthProvider, CratestackContext, CratestackError, RequestContext};
use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{SyncError, SyncServer};
use crate::protocol::SyncErrorCode;

// The routes, which are also the `path` the provider is given: the
// canonical request (ADR 0021 §3) names the declared route, never the URI a
// client sent nor the prefix the router is mounted under.
const REGISTER: &str = "/sync/register";
const PUSH: &str = "/sync/push";
const PULL: &str = "/sync/pull";

#[derive(Clone)]
pub(crate) struct HttpState<A> {
    pub(crate) server: SyncServer,
    pub(crate) auth: A,
}

/// Mount next to the generated router: `app.merge(sync_router(server, auth))`.
pub fn sync_router<A: AuthProvider>(server: SyncServer, auth: A) -> Router {
    Router::new()
        .route(REGISTER, post(register::<A>))
        .route(PUSH, post(push::<A>))
        .route(PULL, post(pull::<A>))
        .route(stream::POKE, get(stream::poke::<A>))
        .with_state(HttpState { server, auth })
}

async fn register<A: AuthProvider>(
    State(state): State<HttpState<A>>,
    request: Request,
) -> Response {
    let result = async {
        let (ctx, body) = state.read(request, REGISTER).await?;
        state.server.register(&ctx, body).await
    };
    respond(result.await)
}

async fn push<A: AuthProvider>(State(state): State<HttpState<A>>, request: Request) -> Response {
    let result = async {
        let (ctx, body) = state.read(request, PUSH).await?;
        state.server.push(&ctx, body).await
    };
    respond(result.await)
}

async fn pull<A: AuthProvider>(State(state): State<HttpState<A>>, request: Request) -> Response {
    let result = async {
        let (ctx, body) = state.read(request, PULL).await?;
        state.server.pull(&ctx, body).await
    };
    respond(result.await)
}

impl<A: AuthProvider> HttpState<A> {
    /// Authenticate, then decode a bounded JSON body.
    async fn read<T: DeserializeOwned>(
        &self,
        request: Request,
        route: &'static str,
    ) -> Result<(CratestackContext, T), SyncError> {
        let (parts, body) = request.into_parts();
        let limit = self.server.policy.max_body_bytes;
        let bytes = to_bytes(body, limit).await.map_err(|_| {
            SyncError::new(
                SyncErrorCode::MutationTooLarge,
                format!("request body exceeds {limit} bytes"),
            )
        })?;
        let ctx = self.authenticate(&parts, route, &bytes).await?;
        let decoded = serde_json::from_slice(&bytes)
            .map_err(|error| SyncError::new(SyncErrorCode::BadRequest, error.to_string()))?;
        Ok((ctx, decoded))
    }

    pub(crate) async fn authenticate(
        &self,
        parts: &Parts,
        route: &'static str,
        body: &Bytes,
    ) -> Result<CratestackContext, SyncError> {
        let request = RequestContext {
            method: parts.method.as_str(),
            path: route,
            query: parts.uri.query(),
            headers: &parts.headers,
            body,
            extensions: &parts.extensions,
        };
        self.auth
            .authenticate(&request)
            .await
            .map_err(|error| refused(error.into()))
    }
}

/// A provider's error as the sync error body; see the module docs.
fn refused(error: CratestackError) -> SyncError {
    let status = error.status_code();
    let code = if status.is_server_error() {
        // The provider could not decide (a key server down): not the
        // caller's fault, so no 401 that would send it to sign in again.
        tracing::error!(error = %error, "sync: the auth provider failed");
        SyncErrorCode::Internal
    } else if status == StatusCode::FORBIDDEN {
        SyncErrorCode::Forbidden
    } else {
        SyncErrorCode::Unauthenticated
    };
    let challenges = error.auth_challenges().to_vec();
    // REST's envelope: its public message (an `Internal` detail stays in
    // the log), and a refusal's `details` and members.
    let envelope = error.into_response();
    let mut body = SyncError::new(code, envelope.message);
    body.details = envelope.details.map(Box::new);
    body.extra = envelope.extra;
    body.challenges = challenges;
    body
}

pub(crate) fn respond<T: Serialize>(result: Result<T, SyncError>) -> Response {
    match result {
        Ok(body) => (StatusCode::OK, axum::Json(body)).into_response(),
        Err(error) => error_response(error),
    }
}

pub(crate) fn error_response(mut error: SyncError) -> Response {
    let status =
        StatusCode::from_u16(error.code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let retry_after = error.retry_after_secs;
    let challenges = std::mem::take(&mut error.challenges);
    let mut response = (status, axum::Json(error)).into_response();
    for challenge in challenges {
        if let Ok(value) = HeaderValue::from_str(&challenge) {
            response.headers_mut().append(WWW_AUTHENTICATE, value);
        }
    }
    if let Some(secs) = retry_after
        && let Ok(value) = secs.to_string().parse()
    {
        response.headers_mut().insert("retry-after", value);
    }
    response
}
