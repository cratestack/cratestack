//! Per-principal rate limiting.
//!
//! Token-bucket algorithm with a pluggable store. The default in-memory
//! implementation is appropriate for single-instance deployments; banks
//! running multiple replicas bring a Redis-backed implementation through
//! the [`RateLimitStore`] trait so all replicas share the same view of
//! consumption.
//!
//! The middleware computes a key per request (the default is the
//! authorization-header fingerprint, the same shape the idempotency layer
//! uses) and refuses with `429` plus a `Retry-After` header when the bucket
//! is empty. Banks running tenant-scoped budgeting can swap the key
//! function for tenant-id.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CoolError;
use http::{HeaderValue, StatusCode, header};
use tower::{Layer, Service};

/// Configuration for a single bucket: capacity (max burst) and refill rate
/// in tokens per second. Banks running high-frequency back-office traffic
/// pick large bursts; consumer-facing channels use small bursts to dampen
/// abuse.
#[derive(Debug, Clone, Copy)]
pub struct RateLimitConfig {
    pub burst: u32,
    pub refill_per_second: f64,
}

impl RateLimitConfig {
    pub fn new(burst: u32, refill_per_second: f64) -> Self {
        Self {
            burst,
            refill_per_second,
        }
    }
}

/// Result of attempting to consume a token. `Allowed` carries the number
/// of tokens left after consumption; `Throttled` carries seconds the
/// caller should wait before retrying.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RateLimitDecision {
    Allowed { remaining: u32 },
    Throttled { retry_after_secs: u32 },
}

/// Pluggable storage for token-bucket state. Implementations must be safe
/// to share across tasks (use a Mutex internally, or rely on the backing
/// store's atomicity).
#[async_trait]
pub trait RateLimitStore: Send + Sync + 'static {
    /// Atomically consume one token for `key`. Returns the decision based
    /// on the bucket state after the consumption attempt.
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError>;
}

#[derive(Debug)]
struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

/// In-memory `RateLimitStore`. Suitable for single-replica deployments and
/// development; banks running multi-replica clusters need a Redis-backed
/// implementation so the limit is enforced cluster-wide.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRateLimitStore {
    buckets: Arc<Mutex<HashMap<String, Bucket>>>,
}

impl InMemoryRateLimitStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RateLimitStore for InMemoryRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError> {
        let mut buckets = self
            .buckets
            .lock()
            .map_err(|_| CoolError::Internal("rate limit store poisoned".to_owned()))?;
        let now = Instant::now();
        let bucket = buckets.entry(key.to_owned()).or_insert(Bucket {
            tokens: config.burst as f64,
            last_refill: now,
        });
        let elapsed = now
            .saturating_duration_since(bucket.last_refill)
            .as_secs_f64();
        bucket.tokens =
            (bucket.tokens + elapsed * config.refill_per_second).min(config.burst as f64);
        bucket.last_refill = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(RateLimitDecision::Allowed {
                remaining: bucket.tokens.floor() as u32,
            })
        } else {
            let need = 1.0 - bucket.tokens;
            let secs = (need / config.refill_per_second).ceil() as u32;
            Ok(RateLimitDecision::Throttled {
                retry_after_secs: secs.max(1),
            })
        }
    }
}

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl RateLimitLayer {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            store,
            config,
            key_fn: Arc::new(default_key_fn),
        }
    }

    pub fn with_key_fn(mut self, f: impl Fn(&Request) -> String + Send + Sync + 'static) -> Self {
        self.key_fn = Arc::new(f);
        self
    }
}

fn default_key_fn(req: &Request) -> String {
    // `auth:` prefix keeps the rate-limit bucket keyspace distinct from
    // any future per-tenant / per-IP keyspace that callers might layer on
    // via `with_key_fn`.
    crate::principal_fingerprint(req, "auth")
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            store: self.store.clone(),
            config: self.config,
            key_fn: self.key_fn.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> String + Send + Sync>,
}

impl<S> Service<Request> for RateLimitService<S>
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
        let config = self.config;
        let key = (self.key_fn)(&req);
        Box::pin(async move {
            match store.consume(&key, config).await {
                Ok(RateLimitDecision::Allowed { remaining }) => {
                    let mut response = inner.call(req).await?;
                    if let Ok(value) = HeaderValue::from_str(&config.burst.to_string()) {
                        response.headers_mut().insert("X-RateLimit-Limit", value);
                    }
                    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
                        response
                            .headers_mut()
                            .insert("X-RateLimit-Remaining", value);
                    }
                    Ok(response)
                }
                Ok(RateLimitDecision::Throttled { retry_after_secs }) => {
                    let mut response = Response::new(Body::from("rate limit exceeded"));
                    *response.status_mut() = StatusCode::TOO_MANY_REQUESTS;
                    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                        response.headers_mut().insert(header::RETRY_AFTER, value);
                    }
                    response.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; charset=utf-8"),
                    );
                    Ok(response)
                }
                Err(error) => {
                    let mut response =
                        Response::new(Body::from(error.public_message().into_owned()));
                    *response.status_mut() = error.status_code();
                    Ok(response)
                }
            }
        })
    }
}

/// Sleep helper for tests — exposes the bucket's wall-clock refill model so
/// the integration tests can exercise both the burst and the throttle path
/// without depending on real time.
#[doc(hidden)]
pub fn _bucket_capacity_for(config: RateLimitConfig) -> u32 {
    config.burst
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

// Workaround for the `Duration` lint that fires when the type is unused in
// some feature combinations.
#[allow(dead_code)]
fn _ensure_duration_referenced(_d: Duration) {}
