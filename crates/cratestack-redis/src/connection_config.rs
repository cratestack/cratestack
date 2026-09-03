//! Bounded [`redis::aio::ConnectionManager`] settings, shared by both
//! stores in this crate (cratestack#846 security review).
//!
//! `redis` defaults **both** `connection_timeout` and `response_timeout`
//! to `None` — unbounded. During a real outage that means every call
//! awaits a full reconnect cycle with no ceiling at all: measured at
//! 9.46s for one attempt, and 18.92s once the rate limiter's retry
//! doubled it. A store call that blocks the request path for nineteen
//! seconds has stopped protecting the service and started harming it,
//! whatever the caller does with the eventual answer.
//!
//! That is a property of the driver's defaults, not of any one store, so
//! both stores use these. The idempotency store gains nothing else from
//! it — it stays fail-closed, since a failed idempotency check must keep
//! failing the request — but it now fails *promptly* rather than after a
//! multi-second hang.
//!
//! These bound the driver only. The rate-limit layer additionally wraps
//! the whole `consume` (first attempt plus retry) in one budget of its
//! own — `cratestack_axum::ratelimit::RateLimitLayer::with_store_timeout`,
//! default 500ms — which is deliberately tighter. Both exist so neither
//! component has to trust the other to be bounded: a store used outside
//! that layer still gets a ceiling, and a layer wrapping some other store
//! still gets one.

use std::time::Duration;

use redis::aio::ConnectionManagerConfig;

/// Ceiling on a single connect attempt.
const CONNECTION_TIMEOUT: Duration = Duration::from_secs(2);

/// Ceiling on waiting for one command's reply.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Deliberately looser than the layer-side budget (see module docs): this
/// is the backstop for a store used on its own, not the request-path SLA.
pub(crate) fn manager_config() -> ConnectionManagerConfig {
    ConnectionManagerConfig::new()
        .set_connection_timeout(Some(CONNECTION_TIMEOUT))
        .set_response_timeout(Some(RESPONSE_TIMEOUT))
}
