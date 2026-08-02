//! `IdempotencyService` — the tower `Service` that owns the per-request
//! state machine (reserve → run → complete/release).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CoolError;
use http::header;
use tower::Service;

use super::complete::buffer_and_persist_response;
use super::hash::{hash_request, is_idempotent_target_method};
use super::parse::parse_idempotency_key;
use super::record::ReservationOutcome;
use super::responses::{error_response, in_flight_response, replay_response};
use super::store::{IdempotencyStore, MAX_BODY_BYTES};
use super::stream_bypass::is_streamed_response;

#[derive(Clone)]
pub struct IdempotencyService<S> {
    pub(super) inner: S,
    pub(super) store: Arc<dyn IdempotencyStore>,
    pub(super) ttl: Duration,
    pub(super) principal_fingerprint: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl<S> Service<Request> for IdempotencyService<S>
where
    S: Service<Request, Response = Response, Error = std::convert::Infallible>
        + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = std::convert::Infallible;
    type Future =
        std::pin::Pin<Box<dyn std::future::Future<Output = Result<Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let store = self.store.clone();
        let ttl = self.ttl;
        let principal_fp = self.principal_fingerprint.clone();
        Box::pin(async move {
            let method = req.method().clone();
            if !is_idempotent_target_method(&method) {
                return inner.call(req).await;
            }
            let key = match parse_idempotency_key(req.headers()) {
                Ok(Some(k)) => k,
                Ok(None) => return inner.call(req).await,
                Err(error) => return Ok(error_response(error)),
            };
            let principal = (principal_fp)(&req);
            // Hash the full path + query string. Skipping the query
            // makes `POST /transfer?dry_run=true` collide with
            // `POST /transfer?dry_run=false` under the same key, so a
            // dry-run preview would replay the live execution's
            // response (or vice versa). Banks routinely encode
            // operation modifiers like `?confirm=true` or
            // `?settlement=instant` in the query string — those must
            // produce distinct idempotency hashes.
            let path = req
                .uri()
                .path_and_query()
                .map(|pq| pq.as_str().to_owned())
                .unwrap_or_else(|| req.uri().path().to_owned());
            let content_type = req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned());

            // Buffer the request body so we can both hash it and replay
            // it into the inner handler.
            let (parts, body) = req.into_parts();
            let bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
                Ok(b) => b,
                Err(_) => {
                    return Ok(error_response(CoolError::BadRequest(
                        "request body exceeds idempotency buffer limit".to_owned(),
                    )));
                }
            };
            let hash = hash_request(&method, &path, content_type.as_deref(), &bytes);

            // Atomic reservation: exactly one caller gets `Reserved`,
            // and only then do we let the handler run. Concurrent
            // callers with the same key + same hash see `InFlight`;
            // different-body conflicts see `Conflict`. This is the
            // banking-grade duplicate-execution guarantee that the
            // previous fetch-then-put pattern could not provide.
            let expires_at = SystemTime::now() + ttl;
            let outcome = match store
                .reserve_or_fetch(&principal, &key, hash, expires_at)
                .await
            {
                Ok(outcome) => outcome,
                Err(error) => return Ok(error_response(error)),
            };

            let token = match outcome {
                ReservationOutcome::Replay(record) => {
                    return Ok(replay_response(&record));
                }
                ReservationOutcome::Conflict => {
                    return Ok(error_response(CoolError::Validation(
                        "idempotency_key_conflict: key reused with a different request body"
                            .to_owned(),
                    )));
                }
                ReservationOutcome::InFlight => {
                    return Ok(in_flight_response());
                }
                ReservationOutcome::Reserved { token } => token,
            };

            // We hold the reservation. Run the handler.
            let req2 = Request::from_parts(parts, Body::from(bytes));
            let response_result = inner.call(req2).await;
            let response = match response_result {
                Ok(response) => response,
                Err(_) => {
                    // `Service::Error = Infallible` so this branch is
                    // unreachable in practice. The release-on-error path
                    // is still here for if/when a fallible inner service
                    // is plugged in. Guarding on `token` ensures a
                    // handler whose reservation has already been
                    // reclaimed (TTL ran out) doesn't drop the new
                    // owner's row.
                    let _ = store.release(&principal, &key, token).await;
                    return Ok(error_response(CoolError::Internal(
                        "handler returned an unrecoverable error".to_owned(),
                    )));
                }
            };
            if is_streamed_response(&response) {
                // Genuinely incremental response (a `@stream` procedure,
                // cratestack#283) — buffering it here would silently
                // defeat streaming, and idempotency-replaying a partial
                // stream has no defined semantics. The handler already
                // ran, so refusing to forward its output would only
                // discard completed work; instead we bypass buffering
                // entirely, release the reservation so a legitimate
                // retry isn't stuck "in flight" forever, and say so
                // loudly. See `super::stream_bypass` for the full
                // rationale.
                let _ = store.release(&principal, &key, token).await;
                tracing::warn!(
                    target: "cratestack",
                    cratestack_operation = "idempotency",
                    "idempotency key supplied for a @stream response body; streaming \
                     responses are not idempotency-replayable — bypassing buffering/replay \
                     for this call (see cratestack#283)",
                );
                return Ok(response);
            }
            Ok(
                buffer_and_persist_response(store.as_ref(), &principal, &key, token, response)
                    .await,
            )
        })
    }
}
