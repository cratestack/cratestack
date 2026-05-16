//! `IdempotencyService` — the tower `Service` that owns the per-request
//! state machine (reserve → run → complete/release).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CoolError;
use http::{StatusCode, header};
use tower::Service;

use super::hash::{hash_request, is_idempotent_target_method};
use super::headers::encode_headers;
use super::parse::parse_idempotency_key;
use super::record::ReservationOutcome;
use super::responses::{error_response, in_flight_response, replay_response};
use super::store::{IdempotencyStore, MAX_BODY_BYTES};

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
            let (rparts, rbody) = response.into_parts();
            let rbytes = match axum::body::to_bytes(rbody, MAX_BODY_BYTES).await {
                Ok(b) => b,
                Err(_) => {
                    // Drop the reservation so retries can attempt
                    // again — but only if our token still holds.
                    let _ = store.release(&principal, &key, token).await;
                    let mut e = error_response(CoolError::Internal(
                        "response body exceeded idempotency buffer".to_owned(),
                    ));
                    *e.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    return Ok(e);
                }
            };
            // Capture the full header set so the replay reproduces the
            // original handler's `Location`, `ETag`, cache directives,
            // `Content-Type`, etc. Hop-by-hop and framework-computed
            // headers are filtered inside `encode_headers`. Pre-fix
            // the middleware only persisted `Content-Type`, so a
            // `201 Created` with a `Location` header replayed as
            // `201 Created` with no `Location` — different observable
            // behaviour from the original execution.
            let headers_blob = encode_headers(&rparts.headers);

            // Persist the completion. Best-effort: on store failure we
            // still return the live response so the caller observes the
            // mutation that DID happen; banks running strict mode can
            // wrap the store in a fail-closed adapter. The `token`
            // guard means a handler whose reservation got reclaimed
            // (TTL expired, retry took over) silently fails this
            // write rather than poisoning the newer reservation's row.
            let _ = store
                .complete(
                    &principal,
                    &key,
                    token,
                    rparts.status.as_u16(),
                    &headers_blob,
                    &rbytes,
                )
                .await;
            Ok(Response::from_parts(rparts, Body::from(rbytes)))
        })
    }
}
