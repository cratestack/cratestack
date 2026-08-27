//! `CRATESTACK_REQUIRE_DB` backend-selection decision — the exact
//! counterpart of `crates/cratestack-pg/tests/support/require_db.rs`, which
//! carries the full rationale and the registry of every other copy.
//!
//! # Why this file exists (cratestack#747)
//!
//! #747 reported the missing-guard defect against `cratestack-pg` only.
//! Auditing the family turned up a *second* site with the identical
//! fall-through: this crate's `connect_or_skip` also threaded `require`
//! into every connection failure and then returned a bare `None` when
//! neither `CRATESTACK_TEST_DATABASE_URL` nor `CRATESTACK_USE_TESTCONTAINERS`
//! was set. `tests/outbox_roundtrip.rs` was therefore skippable in silence
//! with `CRATESTACK_REQUIRE_DB=1` set, exactly like the `cratestack-pg`
//! suites. Fixed here in the same pass rather than left for the next audit
//! to rediscover.
//!
//! Kept as a separate module (rather than inlined in [`super::pg`]) for the
//! same reason as the `cratestack-pg` copy: pure and free of `sqlx`, so
//! `tests/require_guard.rs` can prove the guard is able to fail without
//! mutating process-global env vars.

/// Which PG backend [`super::pg::connect_or_skip`] should use, decided
/// purely from which environment variables are present.
#[derive(Debug, PartialEq, Eq)]
pub enum Backend {
    /// `CRATESTACK_TEST_DATABASE_URL` is set — connect to an external PG.
    Url,
    /// `CRATESTACK_USE_TESTCONTAINERS` is set — spawn an ephemeral one.
    TestContainers,
    /// Neither is set and the caller did not demand a database.
    Skip,
}

/// Pure decision logic for [`super::pg::connect_or_skip`]. Panics when
/// `require` is set but neither backend env var is — see the module docs
/// and the `cratestack-pg` counterpart for why that case must be loud.
///
/// With `require` unset the behaviour is byte-for-byte what it always was;
/// a silent skip stays the deliberate local-dev default.
pub fn pick_backend(has_url: bool, use_testcontainers: bool, require: bool) -> Backend {
    if has_url {
        Backend::Url
    } else if use_testcontainers {
        Backend::TestContainers
    } else if require {
        panic!(
            "CRATESTACK_REQUIRE_DB is set but neither CRATESTACK_TEST_DATABASE_URL nor \
             CRATESTACK_USE_TESTCONTAINERS is set — a misconfigured job would otherwise \
             skip the whole outbox suite silently and still report green"
        );
    } else {
        Backend::Skip
    }
}
