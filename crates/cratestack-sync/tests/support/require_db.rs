//! `CRATESTACK_REQUIRE_DB` backend-selection decision — a copy of
//! `crates/cratestack-pg/tests/support/require_db.rs`, which carries the
//! full rationale (cratestack#747) and the registry of every copy, this one
//! included. Kept pure so `tests/require_guard.rs` can prove the guard is
//! able to fail.

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
             skip the whole sync suite silently and still report green"
        );
    } else {
        Backend::Skip
    }
}
