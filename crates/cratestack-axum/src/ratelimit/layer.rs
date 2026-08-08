use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{ConnectInfo, Request};
use axum::response::Response;
use http::{HeaderValue, StatusCode, header};
use sha2::{Digest, Sha256};
use tower::{Layer, Service};

use super::config::{RateLimitConfig, RateLimitDecision};
use super::store::RateLimitStore;

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

pub(super) fn default_key_fn(req: &Request) -> String {
    // Prefer Authorization header for authenticated requests.
    if let Some(auth_header) = req.headers().get(header::AUTHORIZATION)
        && let Ok(auth_str) = auth_header.to_str()
    {
        let mut h = Sha256::new();
        h.update(auth_str.as_bytes());
        return format!("auth:{:x}", h.finalize());
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
        return format!("ip:{}", addr.ip());
    }

    // Only if both Authorization and a verified peer address are absent
    // (e.g. the server isn't wired through `into_make_service_with_connect_info`),
    // fall back to a single shared bucket. This matches the pre-existing,
    // safe-by-default behavior: unauthenticated traffic is capped in
    // aggregate rather than trusting an unverifiable, attacker-controlled key.
    "anonymous".to_owned()
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
