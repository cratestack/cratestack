use std::net::SocketAddr;
use std::sync::{Arc, Once};

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::response::Response;
use cratestack_core::CratestackError;
use http::{HeaderValue, StatusCode, header};
use sha2::{Digest, Sha256};
use tower::{Layer, Service};

use super::config::{RateLimitConfig, RateLimitDecision};
use super::store::RateLimitStore;

#[derive(Clone)]
pub struct RateLimitLayer {
    store: Arc<dyn RateLimitStore>,
    config: RateLimitConfig,
    key_fn: Arc<dyn Fn(&Request) -> Result<String, CratestackError> + Send + Sync>,
    should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
}

impl RateLimitLayer {
    pub fn new(store: Arc<dyn RateLimitStore>, config: RateLimitConfig) -> Self {
        Self {
            store,
            config,
            key_fn: Arc::new(default_key_fn),
            should_rate_limit_fn: Arc::new(default_should_rate_limit_fn),
        }
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

/// Logged once per process, not per request — see the identical rationale
/// in `idempotency::layer::MISSING_IDENTITY_WARNING`.
static MISSING_IDENTITY_WARNING: Once = Once::new();

/// cratestack#416: the pre-existing default silently collapsed every
/// unauthenticated caller without a verified peer address onto a single
/// shared `"anonymous"` rate-limit bucket — no per-caller throttling at all
/// for that traffic, and one caller could exhaust another's budget. Refusing
/// the request instead makes the gap loud in staging/CI rather than a
/// silently-reachable production bypass.
pub(super) fn default_key_fn(req: &Request) -> Result<String, CratestackError> {
    // Prefer Authorization header for authenticated requests.
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let mut h = Sha256::new();
        h.update(auth_str.as_bytes());
        // sha2 0.11 / digest 0.11 return `hybrid_array::Array`, which (unlike
        // digest 0.10's `GenericArray`) implements no `LowerHex`. The
        // byte-wise `{:02x}` fold below is this repo's existing hex idiom
        // (`cratestack-core/src/transport.rs`) and is byte-for-byte what
        // `format!("{:x}", …)` produced — this string is persisted/keyed on,
        // so it must not change shape.
        let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
        return Ok(format!("auth:{hex}"));
    }

    // Fall back to the real TCP peer address for unauthenticated requests, to
    // avoid collisions between distinct callers. This is deliberately *not*
    // `Forwarded`/`X-Forwarded-For`: those headers are client-supplied and
    // this crate has no trusted-proxy configuration to verify or strip them,
    // so trusting them here would let an attacker mint a fresh rate-limit
    // bucket on every request just by rotating the header value. `ConnectInfo`
    // is populated by axum from the actual accepted socket (when the server
    // is served via `into_make_service_with_connect_info::<SocketAddr>()`)
    // and cannot be spoofed by the client.
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return Ok(format!("ip:{}", addr.ip()));
    }

    // Neither Authorization nor a verified peer address is available (e.g.
    // the server isn't wired through `into_make_service_with_connect_info`).
    // There is no unforgeable value left to key on, so refuse rather than
    // collapsing every such caller onto one shared bucket.
    MISSING_IDENTITY_WARNING.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            "RateLimitLayer's default key function has no Authorization header and no \
             ConnectInfo<SocketAddr> peer on this request, so it cannot verify caller identity. \
             Refusing the request rather than collapsing distinct callers onto a shared \
             \"anonymous\" bucket (cratestack#416) — wire \
             into_make_service_with_connect_info::<SocketAddr>() or supply \
             RateLimitLayer::with_key_fn(...) explicitly. Logged once per process; every \
             matching request is refused until this is fixed.",
        );
    });
    Err(CratestackError::PreconditionFailed(
        "rate limit: no verifiable caller identity (Authorization header or ConnectInfo peer) \
         is available for the default bucket key; the server must be served through \
         into_make_service_with_connect_info::<SocketAddr>() or configure an explicit key \
         function"
            .to_owned(),
    ))
}

/// Default rate limit filter: always rate-limit. Fail closed.
/// Custom filters can check operation descriptors and return false for
/// operations marked `@no_rate_limit` or similar exemptions.
pub(super) fn default_should_rate_limit_fn(_req: &Request) -> bool {
    true
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
                    let mut response =
                        Response::new(Body::from(error.public_message().into_owned()));
                    *response.status_mut() = error.status_code();
                    return Ok(response);
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
                    tracing::warn!(error = %error, "rate limit store error");
                    let mut response =
                        Response::new(Body::from(error.public_message().into_owned()));
                    *response.status_mut() = error.status_code();
                    Ok(response)
                }
            }
        })
    }
}
