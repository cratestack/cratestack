//! The `tower::Service` half of [`super::RateLimitLayer`].
//!
//! Split from `layer.rs` for the workspace's 200-line ceiling when
//! cratestack#871 added the bucket-budget knobs to the builder. The
//! per-request body itself lives one module further out, in
//! `super::consume`.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::response::Response;
use tower::Service;

use super::budget::warn::BudgetWarnings;
use super::config::RateLimitConfig;
use super::layer::KeyFn;
use super::policy::{StoreErrorPolicy, StoreErrorWarnings};
use super::store::RateLimitStore;

#[derive(Clone)]
pub struct RateLimitService<S> {
    pub(super) inner: S,
    pub(super) store: Arc<dyn RateLimitStore>,
    pub(super) config: RateLimitConfig,
    pub(super) key_fn: KeyFn,
    pub(super) should_rate_limit_fn: Arc<dyn Fn(&Request) -> bool + Send + Sync>,
    pub(super) store_error_policy: StoreErrorPolicy,
    pub(super) store_timeout: Duration,
    pub(super) warnings: Arc<StoreErrorWarnings>,
    pub(super) budget_warnings: Arc<BudgetWarnings>,
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
        let should_rate_limit = (self.should_rate_limit_fn)(&req);
        // Clone the whole service, not just `inner`: the async body needs
        // the store, the key fn and both warning budgets, and cloning
        // once is cheaper than seven `Arc::clone`s at the call site.
        let mut service = self.clone();
        Box::pin(async move {
            // If the operation is exempt from rate limiting, skip the check
            // entirely — including key derivation. An exempt route must not
            // be refused just because the default key fn can't verify the
            // caller's identity; only routes that actually need a bucket
            // pay that cost.
            if !should_rate_limit {
                return service.inner.call(req).await;
            }
            Ok(super::consume::run(service, req).await)
        })
    }
}
