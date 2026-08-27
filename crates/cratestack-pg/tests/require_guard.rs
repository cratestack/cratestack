//! Tests that `CRATESTACK_REQUIRE_DB` can actually fail (cratestack#747).
//!
//! A guard that has only ever been observed passing may be *unable* to
//! fail. That is precisely what #747 found here: every `cratestack-pg`
//! PG-backed suite reported `ok` in 0.00s with `CRATESTACK_REQUIRE_DB=1`
//! set and no database anywhere, because `connect_or_skip` threaded
//! `require` into each connection failure but fell through to a bare `None`
//! when neither backend env var was set at all.
//!
//! These run against `support::require_db::pick_backend` — the pure
//! decision — rather than `connect_or_skip` with real env vars, because
//! `std::env::set_var` mutates process-global state shared with every other
//! test thread in the binary and would make the result order-dependent.
//! The pure function is where the bug lived, so it is where the proof
//! belongs.
//!
//! Mirrors `crates/cratestack-redis/tests/require_guard.rs`, which already
//! carried this shape for `CRATESTACK_REQUIRE_REDIS`.

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

/// `just test-pg-tc` and CI's `tests-db` job set `CRATESTACK_REQUIRE_DB=1`
/// *and* `CRATESTACK_USE_TESTCONTAINERS=1` together. That combination must
/// resolve to a real backend, not trip the new guard — the guard is about
/// "no backend configured", not "require is set".
#[test]
fn testcontainers_and_require_together_does_not_panic() {
    assert_eq!(pick_backend(false, true, true), Backend::TestContainers);
}

/// The deliberate local-dev default, unchanged by #747: no flag, no
/// backend, no noise. A contributor without Docker still gets a quiet skip.
#[test]
fn neither_set_and_not_required_skips_quietly() {
    assert_eq!(pick_backend(false, false, false), Backend::Skip);
}

/// The regression test for #747 itself. Delete the `else if require` arm in
/// `support/require_db.rs` and this is the test that fails; nothing else in
/// the workspace would notice.
#[test]
#[should_panic(expected = "CRATESTACK_REQUIRE_DB is set but neither")]
fn neither_set_but_required_panics_instead_of_skipping() {
    let _ = pick_backend(false, false, true);
}
