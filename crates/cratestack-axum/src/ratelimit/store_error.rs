//! The decision, and the logging, for a failed `RateLimitStore::consume`
//! (cratestack#846).
//!
//! Split out of `layer.rs` because it is the security-relevant branch of
//! this middleware and deserves to be readable — and testable — on its
//! own, rather than nested four levels deep inside a boxed future.

use cratestack_core::CratestackError;
use cratestack_core::log_throttle::ThrottleDecision;

use super::policy::{StoreErrorPolicy, StoreErrorWarnings};

/// What the layer should do about a store failure.
pub(super) enum StoreFailure {
    /// Serve the request unthrottled. Reachable only for transport-class
    /// failures under [`StoreErrorPolicy::Allow`].
    Serve,
    /// Refuse, with this error rendered as the usual typed envelope.
    Refuse(CratestackError),
}

/// Log the failure (throttled) and apply the policy.
///
/// The log happens here rather than at the call site so that the
/// "we are serving unthrottled" line cannot drift out of sync with the
/// decision that actually caused it — they are computed together.
pub(super) fn classify_store_failure(
    error: CratestackError,
    policy: StoreErrorPolicy,
    warnings: &StoreErrorWarnings,
) -> StoreFailure {
    let permitted = policy.permits(&error);

    if let ThrottleDecision::Emit {
        suppressed_since_last,
    } = warnings.store_error.check()
    {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            error = %error,
            cratestack_error = error.code(),
            policy = ?policy,
            served_unthrottled = permitted,
            suppressed_since_last,
            "rate limit store error",
        );
    }

    if !permitted {
        return StoreFailure::Refuse(error);
    }

    if let ThrottleDecision::Emit {
        suppressed_since_last,
    } = warnings.fail_open.check()
    {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            suppressed_since_last,
            "RateLimitLayer's store is failing with a TRANSPORT-class error and \
             StoreErrorPolicy::Allow (the default) is in effect, so this request — and every \
             further one while the store stays unreachable — is being served WITHOUT rate \
             limiting. The limiter protects capacity, so an unreachable limiter degrades to \
             unlimited rather than to an outage (cratestack#846). Note this applies ONLY to \
             transport-class failures: a store that is reachable but refusing (OOM, NOPERM, a \
             poisoned mutex) still fails the request under every policy, because such a failure \
             can be caller-induced. Deployments that need even transport failures to refuse \
             must opt in with \
             RateLimitLayer::with_store_error_policy(StoreErrorPolicy::Deny).",
        );
    }
    StoreFailure::Serve
}

#[cfg(test)]
mod tests;
