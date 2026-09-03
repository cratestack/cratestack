//! Shared fixtures for the store-error / timeout / typed-body test
//! modules: one store double per failure class, plus the service and
//! request boilerplate they all need.
//!
//! No log-capturing layer lives here any more — see
//! `super::tests_store_error`'s module docs for why asserting on captured
//! `tracing` events from these suites was scheduling-dependent.

#![cfg(test)]

use std::time::Duration;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision};
use http::header;

use super::store::RateLimitStore;

/// A store that is UNREACHABLE — the transport-class case. This is what
/// `cratestack-redis` reports (as `Unavailable`) for a broken pipe or a
/// refused connection, and the only class `StoreErrorPolicy::Allow`
/// serves through.
pub(super) struct UnreachableStore;

#[async_trait]
impl RateLimitStore for UnreachableStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Err(CratestackError::Unavailable(
            "rate limit store temporarily unavailable".to_owned(),
        ))
    }
}

/// A store that is REACHABLE and refusing — the logical-failure case, and
/// the one the security review showed an unauthenticated caller can
/// induce by rotating `Authorization` until Redis hits `maxmemory`. Must
/// never be served through, under any policy.
pub(super) struct RefusingStore;

#[async_trait]
impl RateLimitStore for RefusingStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        Err(CratestackError::Internal(
            "redis rate limit: OOM command not allowed when used memory > 'maxmemory'".to_owned(),
        ))
    }
}

/// A store that answers, eventually — standing in for the driver's
/// unbounded reconnect cycle (measured at 9.46s per attempt during a real
/// outage, 18.92s with the retry).
pub(super) struct SlowStore {
    pub(super) delay: Duration,
}

#[async_trait]
impl RateLimitStore for SlowStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        tokio::time::sleep(self.delay).await;
        Ok(RateLimitDecision::Allowed { remaining: 0 })
    }
}

/// Buffer a response into `(content_type, body_bytes)`.
pub(super) async fn content_type_and_body(response: Response) -> (String, Vec<u8>) {
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let bytes = to_bytes(response.into_body(), 64 * 1024)
        .await
        .expect("body should buffer");
    (content_type, bytes.to_vec())
}

// --- Service / request / log fixtures shared by the store-error suites ---

async fn ok_handler(_req: Request) -> Result<Response, std::convert::Infallible> {
    Ok(Response::new(Body::from("ok")))
}

pub(super) type OkService = tower::util::ServiceFn<fn(Request) -> OkFuture>;
pub(super) type OkFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = Result<Response, std::convert::Infallible>> + Send>,
>;

/// Spelled with an explicit fn-pointer type rather than an `impl Trait`
/// return: `RateLimitService`'s `Service` impl requires `S::Future: Send`,
/// which an opaque `impl Service` return type does not carry.
pub(super) fn ok_service() -> OkService {
    fn make(req: Request) -> OkFuture {
        Box::pin(ok_handler(req))
    }
    tower::service_fn(make as fn(Request) -> OkFuture)
}

/// A verifiable caller identity, so the request reaches the (failing)
/// store rather than being refused by the default key fn itself
/// (cratestack#416).
pub(super) fn authed_request() -> Request {
    Request::builder()
        .header("authorization", "Bearer test")
        .body(Body::empty())
        .unwrap()
}
