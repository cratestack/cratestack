//! Shared test support for `tests/banking_*.rs`, `tests/policy_db_*.rs`,
//! and any other integration test that needs a real Postgres.
//!
//! This module lives at `tests/support/mod.rs` (rather than as a flat file
//! under `tests/`) so cargo doesn't try to treat it as its own integration
//! test binary. Test files opt in with `mod support;` at the top.

#![allow(dead_code)] // each test binary uses only a subset of these helpers

pub mod pg;
