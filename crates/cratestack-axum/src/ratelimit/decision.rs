//! Shaping the three responses the layer produces once the store has (or
//! has not) answered: allowed, throttled, and "we could not even derive a
//! bucket key".
//!
//! Split out of `layer.rs` to keep it under the workspace's 200-line
//! ceiling after cratestack#846 added the store-error policy and the
//! lookup budget. Behaviour is unchanged; each body moved verbatim.

use axum::extract::Request;
use axum::response::Response;
use cratestack_core::CratestackError;
use http::{HeaderMap, HeaderValue, header};

use crate::middleware_error::middleware_error_response;

use super::config::RateLimitConfig;

/// Advisory budget hints on an allowed response — banks build client-side
/// backoff on these, so they ride along even on the happy path.
pub(super) fn with_budget_headers(
    mut response: Response,
    config: RateLimitConfig,
    remaining: u32,
) -> Response {
    if let Ok(value) = HeaderValue::from_str(&config.burst.to_string()) {
        response.headers_mut().insert("X-RateLimit-Limit", value);
    }
    if let Ok(value) = HeaderValue::from_str(&remaining.to_string()) {
        response
            .headers_mut()
            .insert("X-RateLimit-Remaining", value);
    }
    response
}

/// Expressed as a [`CratestackError`] rather than a hand-built `text/plain`
/// body so the throttle decodes to a typed code in generated clients
/// (`TOO_MANY_REQUESTS` over REST, `resource_exhausted` over RPC) exactly
/// like every other error the stack emits — cratestack#846.
pub(super) fn throttled_response(
    headers: &HeaderMap,
    path: &str,
    retry_after_secs: u32,
) -> Response {
    let mut response = middleware_error_response(
        headers,
        path,
        CratestackError::TooManyRequests("rate limit exceeded".to_owned()),
    );
    if let Ok(value) = HeaderValue::from_str(&retry_after_secs.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// Key derivation is fail-closed under every [`super::StoreErrorPolicy`]
/// (cratestack#416): unlike a store outage, its inputs are
/// caller-controlled, so refusing is the only thing that stops one caller
/// sharing or minting another's bucket.
///
/// Not throttled, unlike the store-error `WARN`: this fires only for a
/// misconfigured deployment (no `Authorization`, no `ConnectInfo`), and
/// the once-per-process explanation in `super::key_fn` already carries
/// the detail. If it turns out to be drivable at volume it should join
/// the throttle in `super::policy`.
pub(super) fn key_failure_response(req: &Request, error: CratestackError) -> Response {
    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "rate_limit",
        error = %error,
        "rate limit key derivation failed",
    );
    middleware_error_response(req.headers(), req.uri().path(), error)
}
