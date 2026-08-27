//! Tests that `CRATESTACK_REQUIRE_DB` can actually fail (cratestack#747).
//!
//! #747 was filed against `cratestack-pg`; the audit it asked for found
//! this crate carrying the identical fall-through, so `outbox_roundtrip.rs`
//! could report `ok` in 0.00s with the guard explicitly enabled and no
//! database anywhere. Same fix, same proof.
//!
//! See `crates/cratestack-pg/tests/require_guard.rs` for the fuller
//! rationale, including why these assert on the pure `pick_backend` rather
//! than driving `connect_or_skip` through real env vars.

mod support;

use support::require_db::Backend;
use support::require_db::pick_backend;

#[test]
fn url_present_wins_regardless_of_testcontainers_or_require() {
    assert_eq!(pick_backend(true, true, true), Backend::Url);
    assert_eq!(pick_backend(true, false, false), Backend::Url);
}

#[test]
fn testcontainers_used_when_url_absent() {
    assert_eq!(pick_backend(false, true, false), Backend::TestContainers);
}

/// `require` plus a configured backend is the normal CI shape and must not
/// trip the guard — it is about "no backend configured", not "require set".
#[test]
fn testcontainers_and_require_together_does_not_panic() {
    assert_eq!(pick_backend(false, true, true), Backend::TestContainers);
}

/// The deliberate local-dev default, unchanged by #747.
#[test]
fn neither_set_and_not_required_skips_quietly() {
    assert_eq!(pick_backend(false, false, false), Backend::Skip);
}

/// The regression test for #747. Delete the `else if require` arm in
/// `support/require_db.rs` and this is the test that fails.
#[test]
#[should_panic(expected = "CRATESTACK_REQUIRE_DB is set but neither")]
fn neither_set_but_required_panics_instead_of_skipping() {
    let _ = pick_backend(false, false, true);
}
