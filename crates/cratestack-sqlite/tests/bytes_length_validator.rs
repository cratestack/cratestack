//! cratestack#572 regression test: `@length` on a `Bytes` field passes
//! `cratestack check` (the parser explicitly permits `Bytes` as an
//! `@length` target — `crates/cratestack-parser/src/validate/validators.rs`)
//! but the emitted `validate()` used to fail `cargo check` with E0308,
//! because `crates/cratestack-macros/src/validators/emit.rs` called
//! `::cratestack::validate_length` unconditionally, and that helper is
//! hard-typed to `&str` while `Bytes` fields generate as `Vec<u8>`
//! (`crates/cratestack-macros/src/shared/types.rs`).
//!
//! Fix: `emit_one` now branches on the field's scalar the same way
//! `emit_range` already does for `Int`/`Decimal`, dispatching `Bytes` to
//! a byte-length-counting sibling, `validate_length_bytes`.
//!
//! Deliberately DB-free: `validate()` is a pure function on the generated
//! input struct, so this test needs no sqlite connection and never skips.

use cratestack::CreateModelInput;
use cratestack::include_embedded_schema;

include_embedded_schema!("tests/fixtures/bytes_length_validator.cstack");

use cratestack_schema::CreateBlobInput;

#[test]
fn digest_shorter_than_min_is_rejected() {
    let input = CreateBlobInput {
        id: 1,
        digest: vec![0u8; 31],
    };
    assert!(input.validate().is_err());
}

#[test]
fn digest_longer_than_max_is_rejected() {
    let input = CreateBlobInput {
        id: 2,
        digest: vec![0u8; 33],
    };
    assert!(input.validate().is_err());
}

#[test]
fn digest_exactly_at_bound_is_accepted() {
    let input = CreateBlobInput {
        id: 3,
        digest: vec![0u8; 32],
    };
    assert!(input.validate().is_ok());
}
