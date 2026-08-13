//! Shared test support for `tests/outbox_roundtrip.rs`.
//!
//! Lives at `tests/support/mod.rs` (not a flat file under `tests/`) so
//! cargo doesn't treat it as its own integration-test binary.

#![allow(dead_code)]

pub mod pg;
