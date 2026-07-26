#![cfg(test)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CoolError;
use http::StatusCode;
use tower::{Layer as TowerLayer, Service};
use tracing_subscriber::Layer as TracingLayer;
use tracing_subscriber::layer::SubscriberExt;

use super::config::{_bucket_capacity_for, RateLimitConfig, RateLimitDecision};
use super::layer::RateLimitLayer;
use super::store::{InMemoryRateLimitStore, RateLimitStore};

#[tokio::test]
async fn allows_up_to_burst_then_throttles() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(3, 0.001); // very slow refill
    for i in 0..3 {
        let decision = store.consume("k", config).await.unwrap();
        assert!(
            matches!(decision, RateLimitDecision::Allowed { .. }),
            "attempt {i} should be allowed: {decision:?}",
        );
    }
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Throttled { .. }));
}

#[tokio::test]
async fn refill_grants_more_tokens_after_wait() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(2, 1000.0); // refills instantly
    // exhaust
    store.consume("k", config).await.unwrap();
    store.consume("k", config).await.unwrap();
    // sleep a hair, then expect refill to allow another
    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    let decision = store.consume("k", config).await.unwrap();
    assert!(matches!(decision, RateLimitDecision::Allowed { .. }));
}

#[tokio::test]
async fn per_key_isolation_does_not_leak_between_principals() {
    let store = InMemoryRateLimitStore::new();
    let config = RateLimitConfig::new(1, 0.001);
    let a = store.consume("alice", config).await.unwrap();
    let b = store.consume("bob", config).await.unwrap();
    assert!(matches!(a, RateLimitDecision::Allowed { .. }));
    assert!(matches!(b, RateLimitDecision::Allowed { .. }));
    let a_throttled = store.consume("alice", config).await.unwrap();
    assert!(matches!(a_throttled, RateLimitDecision::Throttled { .. }));
}

#[test]
fn capacity_helper_passes_burst() {
    assert_eq!(_bucket_capacity_for(RateLimitConfig::new(7, 1.0)), 7);
}

/// A store that always fails, standing in for e.g. an unreachable Redis
/// backend behind `RedisRateLimitStore`.
struct FailingStore;

#[async_trait]
impl RateLimitStore for FailingStore {
    async fn consume(
        &self,
        _key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError> {
        Err(CoolError::Internal(
            "redis rate limit: connection refused".to_owned(),
        ))
    }
}

/// Records every event's level + fields as a formatted string, so a test can
/// assert on log level and content without a full `tracing-subscriber` fmt layer.
#[derive(Default, Clone)]
struct CapturingLayer {
    events: Arc<Mutex<Vec<(tracing::Level, String)>>>,
}

struct FieldsToString(String);

impl tracing::field::Visit for FieldsToString {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if !self.0.is_empty() {
            self.0.push(' ');
        }
        self.0.push_str(&format!("{}={value:?}", field.name()));
    }
}

impl<S: tracing::Subscriber> TracingLayer<S> for CapturingLayer {
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

/// Regression test for a real incident: when the store errors (e.g. Redis
/// unreachable), the layer used to swallow it silently and return a bare
/// 500 with no log line anywhere, making the failure undiagnosable in
/// production. The response must still degrade gracefully, but the error
/// (including the underlying store error text) must be logged — at `WARN`,
/// matching this crate's house convention for every other handled-error
/// log site (`cratestack-macros`' generated procedure/list handlers, the
/// schema-fingerprint mismatch check): `ERROR` is reserved for unhandled
/// conditions, not a store failure that's already been turned into a
/// well-formed error response.
#[test]
fn store_error_is_logged_before_returning_500() {
    let capture = CapturingLayer::default();
    let events = capture.events.clone();
    let subscriber = tracing_subscriber::registry().with(capture);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let status = tracing::subscriber::with_default(subscriber, || {
        rt.block_on(async {
            let layer = RateLimitLayer::new(Arc::new(FailingStore), RateLimitConfig::new(10, 1.0));
            let inner = tower::service_fn(|_req: Request| async {
                Ok::<_, std::convert::Infallible>(Response::new(Body::from("ok")))
            });
            let mut svc = layer.layer(inner);
            let req = Request::builder().body(Body::empty()).unwrap();
            svc.call(req).await.unwrap().status()
        })
    });

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);

    let captured = events.lock().unwrap();
    assert!(
        captured
            .iter()
            .any(|(_, msg)| msg.contains("rate limit store error")),
        "expected a 'rate limit store error' log event, got: {captured:?}"
    );
    assert!(
        captured
            .iter()
            .any(|(_, msg)| msg.contains("redis rate limit: connection refused")),
        "expected the underlying store error text in the log, got: {captured:?}"
    );
    assert!(
        captured
            .iter()
            .any(|(level, msg)| *level == tracing::Level::WARN
                && msg.contains("rate limit store error")),
        "expected the 'rate limit store error' event at WARN (not ERROR), got: {captured:?}"
    );
}
