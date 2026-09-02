use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CratestackError;
use tower::{Layer, Service};

use crate::middleware_error::middleware_error_response;

use super::config::{RateLimitConfig, RateLimitDecision};
use super::decision::{key_failure_response, throttled_response, with_budget_headers};
use super::key_fn::{default_key_fn, default_should_rate_limit_fn};
use super::policy::{
    DEFAULT_STORE_TIMEOUT, StoreErrorPolicy, StoreErrorWarnings, store_timeout_error,
};
use super::store::RateLimitStore;
use super::store_error::{StoreFailure, classify_store_failure};

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
    store_error_policy: StoreErrorPolicy,
    store_timeout: Duration,
    warnings: Arc<StoreErrorWarnings>,
}

impl RateLimitLayer {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            store,
            config,
            key_fn: Arc::new(default_key_fn),
            should_rate_limit_fn: Arc::new(default_should_rate_limit_fn),
            store_error_policy: StoreErrorPolicy::default(),
            store_timeout: DEFAULT_STORE_TIMEOUT,
            warnings: Arc::new(StoreErrorWarnings::default()),
        }
    }

    /// Choose what happens when the backing store itself fails, as
    /// opposed to when a caller is genuinely over budget. Defaults to
    /// [`StoreErrorPolicy::Allow`], which serves through **transport-class
    /// failures only** — see that type's docs for the distinction, why a
    /// reachable-but-refusing store stays closed regardless, and why key
    /// derivation deliberately does not follow suit.
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
        self
    }

    /// Ceiling on how long one store lookup may take before the layer
    /// gives up and applies [`StoreErrorPolicy`] to a synthetic
    /// transport-class error. Defaults to [`DEFAULT_STORE_TIMEOUT`].
    ///
    /// This is ONE budget for the whole lookup, including any retry the
    /// backend performs internally — the point is to bound what the
    /// caller waits, and a per-attempt budget silently doubles when a
    /// store retries. Without it, "degrade to unlimited" degrades only
    /// after the driver's own reconnect cycle finishes, which was
    /// measured at nineteen seconds per request against a real outage.
    pub fn with_store_timeout(mut self, timeout: Duration) -> Self {
        self.store_timeout = timeout;
        self
    }

    /// Override how the layer derives the bucket key. The supplied closure
    /// is infallible by design — opting out of the default's fail-closed
    /// behavior is the caller's explicit choice, including any deliberate
    /// shared bucket.
    pub fn with_key_fn(mut self, f: impl Fn(&Request) -> String + Send + Sync + 'static) -> Self {
        self.key_fn = Arc::new(move |req| Ok(f(req)));
        self
    }

    pub fn with_should_rate_limit_fn(
        mut self,
        f: impl Fn(&Request) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.should_rate_limit_fn = Arc::new(f);
        self
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            store: self.store.clone(),
            config: self.config,
            key_fn: self.key_fn.clone(),
            should_rate_limit_fn: self.should_rate_limit_fn.clone(),
            store_error_policy: self.store_error_policy,
            store_timeout: self.store_timeout,
            warnings: self.warnings.clone(),
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
    store_error_policy: StoreErrorPolicy,
    store_timeout: Duration,
    warnings: Arc<StoreErrorWarnings>,
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
        let key_fn = self.key_fn.clone();
        let store_error_policy = self.store_error_policy;
        let store_timeout = self.store_timeout;
        let warnings = self.warnings.clone();
        let should_rate_limit = (self.should_rate_limit_fn)(&req);
        Box::pin(async move {
            // If the operation is exempt from rate limiting, skip the check
            // entirely — including key derivation. An exempt route must not
            // be refused just because the default key fn can't verify the
            // caller's identity; only routes that actually need a bucket
            // pay that cost.
            if !should_rate_limit {
                return inner.call(req).await;
            }

            let key = match (key_fn)(&req) {
                Ok(key) => key,
                Err(error) => return Ok(key_failure_response(&req, error)),
            };

            // ONE budget for the whole lookup, retry included: the store
            // is free to retry internally, but the caller must not pay
            // for it twice. An elapse is reported as a transport-class
            // error, so it is subject to the same policy as any other
            // "the store did not answer" — cratestack#846.
            let outcome =
                match tokio::time::timeout(store_timeout, store.consume(&key, config)).await {
                    Ok(outcome) => outcome,
                    Err(_elapsed) => Err(store_timeout_error()),
                };

            match outcome {
                Ok(RateLimitDecision::Allowed { remaining }) => Ok(with_budget_headers(
                    inner.call(req).await?,
                    config,
                    remaining,
                )),
                Ok(RateLimitDecision::Throttled { retry_after_secs }) => Ok(throttled_response(
                    req.headers(),
                    req.uri().path(),
                    retry_after_secs,
                )),
                Err(error) => match classify_store_failure(error, store_error_policy, &warnings) {
                    StoreFailure::Serve => inner.call(req).await,
                    StoreFailure::Refuse(error) => Ok(middleware_error_response(
                        req.headers(),
                        req.uri().path(),
                        error,
                    )),
                },
            }
        })
    }
}
