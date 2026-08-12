//! cratestack#537 regression test, server-schema (`include_server_schema!`)
//! counterpart of `cratestack-sqlite/tests/nullable_update_validator.rs` —
//! see that file's module doc for the full mechanism and design decision.
//!
//! `model/inputs.rs::generate_update_input_struct`/`generate_create_input_struct`
//! are the exact same functions `include/server/collect/models.rs` and
//! `include/embedded.rs` both call, so the fix in
//! `cratestack-macros/src/validators/emit.rs` is a single shared fix, but
//! this test proves it concretely on the server path too rather than
//! relying on that being obvious from reading the code.
//!
//! Deliberately DB-free: `validate()` is a pure function on the generated
//! input struct, so this test needs no live/testcontainer Postgres and
//! never skips (unlike this crate's `banking_*` integration tests, which
//! skip silently without `CRATESTACK_TEST_DATABASE_URL` /
//! `CRATESTACK_USE_TESTCONTAINERS`).

use cratestack::include_server_schema;
use cratestack::{CreateModelInput, UpdateModelInput};

include_server_schema!(
    "tests/fixtures/nullable_update_validator.cstack",
    db = Postgres
);

use cratestack_schema::{CreateNoteInput, UpdateNoteInput};

#[test]
fn create_input_with_nullable_validated_fields_still_validates() {
    let ok = CreateNoteInput {
        id: 1,
        body: Some("hello".to_owned()),
        priority: Some(3),
    };
    assert!(ok.validate().is_ok());

    let bad = CreateNoteInput {
        id: 2,
        body: Some(String::new()), // length 0, below min: 1
        priority: Some(3),
    };
    assert!(bad.validate().is_err());
}

#[test]
fn update_input_field_wholly_omitted_skips_validation() {
    let input = UpdateNoteInput {
        body: None,
        priority: None,
    };
    assert!(input.validate().is_ok());
}

#[test]
fn update_input_explicit_null_skips_validation() {
    // `Some(None)`: caller explicitly wants to set the column to NULL —
    // per the design decision, the validator does not run.
    let input = UpdateNoteInput {
        body: Some(None),
        priority: Some(None),
    };
    assert!(input.validate().is_ok());
}

#[test]
fn update_input_new_value_runs_validators() {
    let valid = UpdateNoteInput {
        body: Some(Some("hello".to_owned())),
        priority: Some(Some(3)),
    };
    assert!(valid.validate().is_ok());

    let bad_length = UpdateNoteInput {
        body: Some(Some(String::new())), // length 0, below min: 1
        priority: Some(Some(3)),
    };
    assert!(bad_length.validate().is_err());

    let bad_range = UpdateNoteInput {
        body: Some(Some("hello".to_owned())),
        priority: Some(Some(9)), // above max: 5
    };
    assert!(bad_range.validate().is_err());
}
