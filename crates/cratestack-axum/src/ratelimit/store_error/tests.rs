//! The security-relevant table: which store failures may be served
//! unthrottled, under which policy (cratestack#846 security review).

use cratestack_core::CratestackError;

use super::super::policy::{StoreErrorPolicy, StoreErrorWarnings};
use super::{StoreFailure, classify_store_failure};

fn is_served(error: CratestackError, policy: StoreErrorPolicy) -> bool {
    matches!(
        classify_store_failure(error, policy, &StoreErrorWarnings::default()),
        StoreFailure::Serve
    )
}

/// The only case that may bypass the limiter: the store could not be
/// reached at all. Nothing a caller sends produces this, and it self-heals.
#[test]
fn transport_class_is_served_under_allow() {
    assert!(is_served(
        CratestackError::Unavailable("rate limit store temporarily unavailable".to_owned()),
        StoreErrorPolicy::Allow,
    ));
}

/// The measured attack: rotate an unvalidated `Authorization` header to
/// mint one Redis key per request until the instance hits `maxmemory`,
/// at which point every `HSET` fails with `OOM` — which `cratestack-redis`
/// reports as `Internal`, not `Unavailable`. If `Allow` served through
/// that, any unauthenticated caller could disable the limiter globally.
#[test]
fn a_reachable_but_refusing_store_is_never_served_even_under_allow() {
    assert!(!is_served(
        CratestackError::Internal("redis rate limit: OOM command not allowed".to_owned()),
        StoreErrorPolicy::Allow,
    ));
}

/// A poisoned mutex in `InMemoryRateLimitStore` is a logical failure, not
/// a transport one — there is no connection to heal — so it must not open
/// the gate either.
#[test]
fn a_poisoned_in_memory_store_is_not_served_under_allow() {
    assert!(!is_served(
        CratestackError::Internal("rate limit store poisoned".to_owned()),
        StoreErrorPolicy::Allow,
    ));
}

/// Any other logical refusal the backend might produce stays closed too —
/// the rule is an allowlist of one variant, not a denylist of known-bad
/// messages.
#[test]
fn other_logical_failures_stay_closed_under_allow() {
    for error in [
        CratestackError::Forbidden("NOPERM".to_owned()),
        CratestackError::Codec("malformed reply".to_owned()),
        CratestackError::Database("wrong type".to_owned()),
    ] {
        assert!(
            !is_served(error, StoreErrorPolicy::Allow),
            "only Unavailable may be served through"
        );
    }
}

/// `Deny` is unconditional: even a transport failure refuses.
#[test]
fn deny_refuses_transport_class_too() {
    assert!(!is_served(
        CratestackError::Unavailable("rate limit store temporarily unavailable".to_owned()),
        StoreErrorPolicy::Deny,
    ));
}

/// The refusal carries the store's own error through unchanged, so the
/// status the caller sees (503 for unreachable, 500 for a refusing store)
/// distinguishes the two cases without leaking driver detail.
#[test]
fn the_refusal_preserves_the_original_error() {
    let StoreFailure::Refuse(error) = classify_store_failure(
        CratestackError::Internal("redis rate limit: OOM command not allowed".to_owned()),
        StoreErrorPolicy::Allow,
        &StoreErrorWarnings::default(),
    ) else {
        panic!("OOM must be refused under Allow");
    };
    assert_eq!(error.code(), "INTERNAL_ERROR");

    let StoreFailure::Refuse(error) = classify_store_failure(
        CratestackError::Unavailable("rate limit store temporarily unavailable".to_owned()),
        StoreErrorPolicy::Deny,
        &StoreErrorWarnings::default(),
    ) else {
        panic!("Deny must refuse");
    };
    assert_eq!(error.code(), "UNAVAILABLE");
}
