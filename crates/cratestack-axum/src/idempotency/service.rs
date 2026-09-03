//! `IdempotencyService` — the tower `Service` that owns the per-request
//! state machine (admit → run → complete/release).
//!
//! Since ADR 0015 slice 1 the *decision* is not made here: this is the
//! HTTP adapter that assembles a `cratestack_exec::OpInput` out of things
//! only a transport knows — the `Idempotency-Key` header, the method,
//! the path + query, the content-type, the buffered body — and asks
//! `OpExecutor::admit`. Everything transport-shaped stayed put on
//! purpose; nothing about the bytes on the wire changed.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CratestackError;
use cratestack_exec::{OpAdmission, OpExecutor};
use http::header;
use tower::Service;

use crate::middleware_error::middleware_error_response;

use super::finish::finish_response;
use super::hash::{hash_request, is_idempotent_target_method};
use super::parse::parse_idempotency_key;
use super::reserve::{Reservation, admit_or_response};
use super::store::MAX_BODY_BYTES;

#[derive(Clone)]
pub struct IdempotencyService<S> {
    pub(super) inner: S,
    pub(super) executor: OpExecutor,
    pub(super) principal_fingerprint:
        Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    pub(super) op_resolver: Arc<dyn Fn(&Request) -> OpAdmission + Send + Sync>,
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
        let executor = self.executor.clone();
        let principal_fp = self.principal_fingerprint.clone();
        let op_resolver = self.op_resolver.clone();
        Box::pin(async move {
            let method = req.method().clone();
            if !is_idempotent_target_method(&method) {
                return inner.call(req).await;
            }
            let key = match parse_idempotency_key(req.headers()) {
                Ok(Some(k)) => k,
                Ok(None) => return inner.call(req).await,
                Err(error) => {
                    return Ok(middleware_error_response(
                        req.headers(),
                        req.uri().path(),
                        error,
                    ));
                }
            };
            // Resolve the op BEFORE anything else touches the request:
            // both resolvers need only the method and the path (plus
            // `MatchedPath` from the extensions), none of which survives
            // `into_parts` below. With no resolver installed this is
            // `OpAdmission::unresolved()`, whose `idempotent_by_default`
            // is `false` — so the short-circuit below never fires and the
            // path through this function is bit-for-bit the pre-ADR-0015
            // one.
            let op = (op_resolver)(&req);
            if op.idempotent_by_default {
                // The op does not participate: `admit` would answer
                // `Bypass`, and everything between here and there exists
                // only to build that answer's argument. Returning now,
                // rather than after buffering, makes a bypassed request
                // behave exactly like one that sent no `Idempotency-Key`
                // at all — otherwise a `@no_idempotency` POST paid the
                // 2 MiB request cap to compute a fingerprint nobody reads
                // (and was refused at 2 MiB + 1 for carrying a header its
                // own schema says does nothing), and could be refused
                // outright by the principal fingerprint (cratestack#416)
                // for want of a namespace it will never use.
                return inner.call(req).await;
            }
            let principal = match (principal_fp)(&req) {
                Ok(principal) => principal,
                Err(error) => {
                    return Ok(middleware_error_response(
                        req.headers(),
                        req.uri().path(),
                        error,
                    ));
                }
            };
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
            // Kept alive past the `Request::from_parts` below because
            // every error response this middleware emits after that point
            // still has to negotiate its content type against the
            // *request's* headers (cratestack#846). One `HeaderMap` clone
            // per idempotent request; the alternative — reconstructing a
            // headers-shaped value at four call sites — costs more in
            // clarity than this does in allocation.
            let error_headers = parts.headers.clone();
            let error_path = parts.uri.path().to_owned();
            let bytes = match axum::body::to_bytes(body, MAX_BODY_BYTES).await {
                Ok(b) => b,
                Err(_) => {
                    return Ok(middleware_error_response(
                        &error_headers,
                        &error_path,
                        CratestackError::BadRequest(
                            "request body exceeds idempotency buffer limit".to_owned(),
                        ),
                    ));
                }
            };
            let hash = hash_request(&method, &path, content_type.as_deref(), &bytes);

            // Atomic reservation: exactly one caller gets `Reserved`,
            // and only then do we let the handler run. Concurrent
            // callers with the same key + same hash see `InFlight`;
            // different-body conflicts see `Conflict`. This is the
            // banking-grade duplicate-execution guarantee that the
            // previous fetch-then-put pattern could not provide. The
            // decision itself is L3's (ADR 0015 slice 1); the four
            // outcomes and their responses are unchanged.
            let token = match admit_or_response(
                &executor,
                op,
                &principal,
                &key,
                hash,
                &error_headers,
                &error_path,
            )
            .await
            {
                Reservation::Held(token) => Some(token),
                // The op opted out (`@no_idempotency`, or a read): run
                // it, but there is no reservation to complete or release
                // afterwards.
                Reservation::Bypass => None,
                Reservation::Finished(response) => return Ok(response),
            };

            // Run the handler, then dispose of its response
            // (release / forward / persist) in `finish_response`.
            let req2 = Request::from_parts(parts, Body::from(bytes));
            let response_result = inner.call(req2).await;
            Ok(finish_response(
                &executor,
                token,
                &principal,
                &key,
                response_result,
                &error_headers,
                &error_path,
            )
            .await)
        })
    }
}
