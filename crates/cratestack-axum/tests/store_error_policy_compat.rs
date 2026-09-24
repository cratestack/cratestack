//! cratestack#1038 moved `StoreErrorPolicy` and `DEFAULT_STORE_TIMEOUT`
//! from this crate to `cratestack-exec` (L3), so MCP takes the same setting
//! as HTTP. This crate re-exports both from their old path; this file is
//! the proof that the move is source-compatible for a downstream crate.
//!
//! The first test is written the way user code was written before the
//! move: it names only `cratestack_axum::ratelimit`, and uses every
//! capability the type had in public (its variants, `Default`, `Copy`,
//! `PartialEq`, `Debug`, a `match` with the wildcard `#[non_exhaustive]`
//! requires, and the constant in a `const` context). If the re-export were
//! dropped or narrowed, this file would stop compiling. The second pins
//! that the old path names the L3 type itself, not a lookalike copy that
//! would let the two transports' settings drift apart again.

use std::any::TypeId;
use std::sync::Arc;
use std::time::Duration;

use cratestack_axum::ratelimit::{
    DEFAULT_STORE_TIMEOUT, InMemoryRateLimitStore, RateLimitConfig, RateLimitLayer,
    StoreErrorPolicy,
};

const BUDGET: Duration = DEFAULT_STORE_TIMEOUT;

fn describe(policy: StoreErrorPolicy) -> &'static str {
    match policy {
        StoreErrorPolicy::Allow => "allow",
        StoreErrorPolicy::Deny => "deny",
        _ => "unknown",
    }
}

#[test]
fn pre_move_user_code_still_compiles_and_behaves() {
    let chosen = StoreErrorPolicy::Deny;
    let copied = chosen;
    assert_eq!(chosen, copied);
    assert_eq!(StoreErrorPolicy::default(), StoreErrorPolicy::Allow);
    assert_eq!(format!("{chosen:?}"), "Deny");
    assert_eq!(describe(chosen), "deny");
    assert_eq!(BUDGET, Duration::from_millis(500));

    let _layer = RateLimitLayer::new(
        Arc::new(InMemoryRateLimitStore::new()),
        RateLimitConfig::new(10, 1.0),
    )
    .with_store_error_policy(chosen)
    .with_store_timeout(DEFAULT_STORE_TIMEOUT);
}

#[test]
fn the_old_path_names_the_l3_type_not_a_copy() {
    assert_eq!(
        TypeId::of::<StoreErrorPolicy>(),
        TypeId::of::<cratestack_exec::StoreErrorPolicy>(),
    );
    assert_eq!(
        DEFAULT_STORE_TIMEOUT,
        cratestack_exec::DEFAULT_STORE_TIMEOUT
    );
}
