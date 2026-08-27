//! `CRATESTACK_REQUIRE_DB` backend-selection decision, split out from
//! [`super::pg`] so it is pure (no I/O, no `sqlx`) and therefore
//! deterministically unit-testable — see `tests/require_guard.rs`.
//!
//! # Why this is its own module (cratestack#747)
//!
//! `CRATESTACK_REQUIRE_DB` exists to convert a silent skip into a loud
//! failure, so that a green run can be trusted as evidence. Until #747 it
//! did not do that in this crate: `require` was threaded through every
//! *connection failure* but ignored on the path that matters most — neither
//! `CRATESTACK_TEST_DATABASE_URL` nor `CRATESTACK_USE_TESTCONTAINERS` set at
//! all, which is the single most likely CI misconfiguration. The whole
//! `cratestack-pg` PG-backed suite printed `ok` in 0.00s having touched no
//! database, with the guard explicitly enabled, and three separate reviews
//! accepted that green as proof.
//!
//! The decision is pure precisely so the guard can be *proven able to fail*
//! by a `#[should_panic]` test rather than only ever observed passing. The
//! alternative — asserting on `connect_or_skip` with real env vars — would
//! mutate process-global state and race the other tests sharing the binary.
//!
//! # The other copies of this guard
//!
//! The same idea is implemented independently in six other places, because
//! integration-test support modules compile as separate crates and each
//! site reaches `sqlx` by a different path (`cratestack::sqlx`,
//! `cratestack_sqlx::sqlx`, bare `sqlx_postgres`). #747 found two of the
//! seven ignoring `require` on this path — this crate and
//! `cratestack-outbox`. Keep them in sync:
//!
//! - `crates/cratestack-outbox/tests/support/require_db.rs` (was the other
//!   broken one; same split, same tests)
//! - `crates/cratestack-studio/tests/support/pg.rs`
//! - `crates/cratestack-migrate/tests/postgres_introspect.rs`
//! - `crates/cratestack-cli/src/migrate/tests_baseline.rs` (`base_url`)
//! - `crates/cratestack-redis/tests/support/redis.rs` (`CRATESTACK_REQUIRE_REDIS`)
//! - `crates/cratestack-redis/src/test_support.rs` (`CRATESTACK_REQUIRE_REDIS`)
//!
//! `examples/db-transaction-verification/tests/transaction.rs` reads
//! `CRATESTACK_REQUIRE_DB` too, but has no fall-through to guard: it always
//! uses testcontainers, so "neither configured" is not a reachable state
//! there.
//!
//! A shared helper crate was considered and deliberately not built — see
//! the CHANGELOG entry for #747 for the reasoning (a new crate under
//! `crates/` must be assigned a layer in `docs/adr/layers.toml`, and
//! test-only scaffolding has no honest place in the L0-L5 model).

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

/// Pure decision logic for [`super::pg::connect_or_skip`].
///
/// Panics in the one case the loud-failure guard exists to catch:
/// `require` is set (CI opted into `CRATESTACK_REQUIRE_DB`) but neither
/// backend env var is. Panicking here — rather than returning
/// [`Backend::Skip`] as this did before #747 — is the only thing that makes
/// `CRATESTACK_REQUIRE_DB` mean what its name claims.
///
/// Priority order is unchanged: an explicit URL wins (most useful for "I
/// have this thing already running"), testcontainers second, skip last.
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
             skip the whole Postgres suite silently and still report green"
        );
    } else {
        Backend::Skip
    }
}
