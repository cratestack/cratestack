//! Shared fixtures for the store-error/typed-body test modules: a store
//! that always fails, and a `tracing` layer that captures events so a
//! test can assert on log level and content without a full fmt layer.
//!
//! Moved verbatim out of `tests.rs` when cratestack#846 split the
//! store-error tests into their own module.

#![cfg(test)]

use std::sync::{Arc, Mutex};
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

/// Records every event's level + fields as a formatted string, so a test can
/// assert on log level and content without a full `tracing-subscriber` fmt layer.
#[derive(Default, Clone)]
pub(super) struct CapturingLayer {
    pub(super) events: Arc<Mutex<Vec<(tracing::Level, String)>>>,
}

pub(super) struct FieldsToString(pub(super) String);

impl tracing::field::Visit for FieldsToString {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&format!("{}={value:?}", field.name()));
    }
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for CapturingLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = FieldsToString(String::new());
        event.record(&mut visitor);
        self.events
            .lock()
            .unwrap()
            .push((*event.metadata().level(), visitor.0));
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

pub(super) type CapturedEvents = Arc<Mutex<Vec<(tracing::Level, String)>>>;

/// `#[tokio::test]` drives a current-thread runtime, so the thread-local
/// default subscriber this guard installs stays in effect across the
/// `.await` points in the callers — no separate runtime needed. The
/// throttles the layer logs through are per-layer (see `super::policy`),
/// so a fresh layer per test keeps these assertions order-independent.
pub(super) fn capture_logs() -> (tracing::subscriber::DefaultGuard, CapturedEvents) {
    use tracing_subscriber::layer::SubscriberExt;

    let capture = CapturingLayer::default();
    let events = capture.events.clone();
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    (guard, events)
}
