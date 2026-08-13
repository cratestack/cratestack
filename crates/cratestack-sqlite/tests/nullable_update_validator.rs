//! cratestack#537 regression test: a validator attribute (`@length`,
//! `@range`, …) on a nullable field used to make the generated
//! `Update{Model}Input::validate()` fail to compile.
//!
//! Mechanism: update inputs wrap every field in `Option<T>` ("field
//! omitted" — don't touch this column), and a nullable column is
//! *separately* `Option<T>` ("set this column to NULL"). The two are
//! independent, so `note.body: String? @length(...)` becomes
//! `Option<Option<String>>` on `UpdateNoteInput`. Before the fix,
//! `cratestack-macros/src/validators/emit.rs` OR'd the two conditions into
//! a single boolean and unwrapped exactly once, so the generated
//! `validate()` body called `validate_length(name, value, ..)` with
//! `value: &Option<String>` where a `&str` was expected — a compile
//! error inside macro-generated code, at the `include_embedded_schema!`
//! call site, not at any one field.
//!
//! `Create{Model}Input` was never affected — a nullable field there is a
//! single `Option<T>`, and `treat_as_optional` is only `true` for updates.
//!
//! This fixture exercises both `@length` (on `String?`) and `@range` (on
//! `Int?`) since the bug is in the shared unwrap-depth logic in `emit.rs`,
//! not in any one validator helper. `include_server_schema!` and
//! `include_embedded_schema!` both compose `Create`/`Update{Model}Input`
//! from the exact same `cratestack_macros::model::inputs` functions (see
//! `include/server/collect/models.rs` and `include/embedded.rs`), so a fix
//! at this shared emission site covers both entry macros — this crate
//! exercises the embedded path because it needs no external Postgres
//! (in-memory sqlite, synchronous, never skips in CI).
//!
//! Design decision (stated in cratestack#537): validating an explicit
//! "set this column to NULL" (`Some(None)` on the doubly-wrapped field) is
//! a no-op — the validator does not run, mirroring how a nullable field on
//! `Create{Model}Input` is *allowed* to be absent/null in the first place.
//! Only `Some(Some(value))` — a genuine new value supplied on update — runs
//! the validator. A wholly omitted field (`None`) is unaffected, as before.

use cratestack::include_embedded_schema;
use cratestack::{CreateModelInput, UpdateModelInput};

include_embedded_schema!("tests/fixtures/nullable_update_validator.cstack");

use cratestack_schema::{CreateNoteInput, UpdateNoteInput};

#[test]
fn create_input_with_nullable_validated_fields_still_validates() {
    // Single-Option case (nullable, not "treat as optional") — this always
    // worked, kept here as a control so a future regression in the
    // opposite direction (breaking create) shows up too.
    let ok = CreateNoteInput {
        id: 1,
        body: Some("hello".to_owned()),
        priority: Some(3),
    };
    assert!(ok.validate().is_ok());

    let ok_absent = CreateNoteInput {
        id: 2,
        body: None,
        priority: None,
    };
    assert!(ok_absent.validate().is_ok());

    let bad = CreateNoteInput {
        id: 3,
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
    // `Some(None)`: the caller explicitly wants to set the column to NULL.
    // Per the design decision above, the validator does not run — an
    // explicit NULL is not a value to range/length-check.
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
