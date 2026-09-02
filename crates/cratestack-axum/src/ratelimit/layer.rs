use std::sync::Arc;

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CratestackError;
use http::{HeaderValue, header};
use tower::{Layer, Service};

use crate::middleware_error::middleware_error_response;

use super::config::{RateLimitConfig, RateLimitDecision};
use super::key_fn::{default_key_fn, default_should_rate_limit_fn};
use super::policy::{StoreErrorPolicy, warn_fail_open_once};
use super::store::RateLimitStore;

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
    store_error_policy: StoreErrorPolicy,
}

impl RateLimitLayer {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            store,
            config,
            key_fn: Arc::new(default_key_fn),
            should_rate_limit_fn: Arc::new(default_should_rate_limit_fn),
            store_error_policy: StoreErrorPolicy::default(),
        }
    }

    /// Choose what happens when the backing store itself fails, as
    /// opposed to when a caller is genuinely over budget. Defaults to
    /// [`StoreErrorPolicy::Allow`] (fail open) — see that type's docs for
    /// why, and for why key derivation deliberately does *not* follow
    /// suit.
    pub fn with_store_error_policy(mut self, policy: StoreErrorPolicy) -> Self {
        self.store_error_policy = policy;
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
                Err(error) => {
                    tracing::warn!(
                        target: "cratestack",
                        cratestack_operation = "rate_limit",
                        error = %error,
                        "rate limit key derivation failed",
                    );
                    return Ok(middleware_error_response(
                        req.headers(),
                        req.uri().path(),
                        error,
                    ));
                }
            };

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
                    // Expressed as a `CratestackError` rather than a
                    // hand-built `text/plain` body so the throttle
                    // decodes to a typed code in generated clients
                    // (`TOO_MANY_REQUESTS` over REST,
                    // `resource_exhausted` over RPC) exactly like every
                    // other error the stack emits — cratestack#846.
                    let mut response = middleware_error_response(
                        req.headers(),
                        req.uri().path(),
                        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
                    );
                    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
                        response.headers_mut().insert(header::RETRY_AFTER, value);
                    }
                    Ok(response)
                }
                Err(error) => {
                    // Unconditional and per-request on purpose: an
                    // operator needs the failure *rate*, and under
                    // `Allow` this line is the only trace that the
                    // limiter is not actually limiting.
                    tracing::warn!(
                        target: "cratestack",
                        cratestack_operation = "rate_limit",
                        error = %error,
                        policy = ?store_error_policy,
                        "rate limit store error",
                    );
                    match store_error_policy {
                        StoreErrorPolicy::Allow => {
                            warn_fail_open_once();
                            inner.call(req).await
                        }
                        StoreErrorPolicy::Deny => Ok(middleware_error_response(
                            req.headers(),
                            req.uri().path(),
                            error,
                        )),
                    }
                }
            }
        })
    }
}
