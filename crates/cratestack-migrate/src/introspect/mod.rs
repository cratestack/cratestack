//! Live-database schema introspection.
//!
//! Phase B of the migration-baselining plan (issue #204, epic #202,
//! `docs/design/migrate-baseline.md` §5.2 and §7's Phase B row). Phase A
//! (issue #203, merged) split `cratestack-migrate`'s diff engine into a
//! `Schema → Projections` projection step
//! ([`crate::project`]) and a pure `Projections → Projections`
//! comparison ([`crate::diff_projections`]). This module is the second,
//! non-`Schema` way to produce a [`crate::Projections`] value: instead
//! of reading a parsed `.cstack` file, [`postgres::introspect`] reads a
//! *live database*'s catalog state and produces the same IR shape, so
//! a future baseline command (Phase C, issue #205) can diff "what the
//! schema says" against "what the database actually has" through the
//! exact same [`crate::diff_projections`] entry point.
//!
//! Only Postgres is supported (design doc §6, open question 2 — no
//! long-lived "existing production database" adoption story exists for
//! SQLite/embedded targets today).

pub mod postgres;
