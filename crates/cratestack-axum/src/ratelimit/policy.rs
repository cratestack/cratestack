//! What the rate-limit layer does when the *store* itself fails
//! (cratestack#846).

use std::sync::Once;

/// How [`super::RateLimitLayer`] treats a failure of the backing
/// [`super::RateLimitStore`] — a Redis outage, a dropped connection, a
/// poisoned in-memory mutex — as distinct from a caller who is genuinely
/// over budget.
///
/// # Why the default is [`StoreErrorPolicy::Allow`]
///
/// Key derivation stays fail-closed (cratestack#416): its inputs are
/// *caller-controlled*, so refusing is the only way to stop one caller
/// from minting or sharing another's bucket. A store outage is the
/// opposite — no caller caused it and no caller can fix it, and failing
/// closed converts a limiter hiccup into a total outage of every
/// rate-limited route at once. The limiter exists to protect capacity;
/// when the limiter is broken the protection is simply absent, so the
/// correct degradation is "unlimited", not "nothing works". This is what
/// gateways (Envoy's `failure_mode_deny: false`, nginx, Kong) default to
/// for the same reason.
///
/// Deployments where the limiter is a *security* control rather than a
/// capacity control — a paywall, a brute-force guard — want the other
/// answer and should say so explicitly with
/// [`super::RateLimitLayer::with_store_error_policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum StoreErrorPolicy {
    /// Log the failure and let the request through unthrottled.
    #[default]
    Allow,
    /// Refuse the request with the store's own error status (a 500 for
    /// the `Internal` errors both shipped stores produce), carrying the
    /// normal typed error envelope.
    Deny,
}

/// Logged once per process, not per request — mirrors
/// `layer::MISSING_IDENTITY_WARNING`. The per-request `warn!` in
/// `RateLimitService::call` stays unconditional (an operator needs the
/// failure *rate*, and the tests assert on it); this one carries the
/// long "you are now serving unthrottled" explanation that would be pure
/// noise repeated per request.
static FAIL_OPEN_NOTICE: Once = Once::new();

pub(super) fn warn_fail_open_once() {
    FAIL_OPEN_NOTICE.call_once(|| {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            "RateLimitLayer's store failed and StoreErrorPolicy::Allow (the default) is in \
             effect, so this request — and every further request while the store stays down — \
             is being served WITHOUT rate limiting. The limiter protects capacity, so a broken \
             limiter degrades to unlimited rather than to an outage (cratestack#846). \
             Deployments that need the opposite (a limiter used as a security control) must opt \
             in with RateLimitLayer::with_store_error_policy(StoreErrorPolicy::Deny). Logged \
             once per process; the per-request \"rate limit store error\" WARN below is not \
             suppressed.",
        );
    });
}
