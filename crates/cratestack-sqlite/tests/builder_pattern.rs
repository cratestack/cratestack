//! Positive proof that the typestate builder every struct-shaped generated
//! type gets (`cratestack-core/src/builder.rs`, `cratestack-macros/src/
//! builder.rs`) produces output byte-for-byte equal to the equivalent
//! struct literal — for the model struct, the view struct, and the
//! `Create{Model}Input`/`Update{Model}Input` pair, including all three
//! double-option patch states (cratestack#567: untouched / set / explicitly
//! cleared).
//!
//! Fully DB-free (in-memory sqlite, `include_embedded_schema!`) — every
//! assertion here is a plain struct comparison, none of it touches a
//! connection, so nothing in this file can silently skip.
//!
//! The *negative* half of the guarantee ("omitting a required field is a
//! compile error, not a `Result`") is a `trybuild` UI case instead of
//! anything in this file — see `crates/cratestack-macros/tests/
//! ui_builder_required_field.rs` for that proof and for why a compile-fail
//! assertion has to live in a standalone crate rather than here.

use cratestack::include_embedded_schema;

include_embedded_schema!("tests/fixtures/builder_pattern.cstack");

use cratestack_schema::models::{BuilderWidget, BuilderWidgetSummary};
use cratestack_schema::{CreateBuilderWidgetInput, UpdateBuilderWidgetInput};

// ───── #1 Create input: a mix of required + optional fields ─────────────

#[test]
fn create_input_builder_matches_struct_literal_with_optional_field_set() {
    let built = CreateBuilderWidgetInput::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .note(Some("has a note".to_owned()))
        .build();
    let literal = CreateBuilderWidgetInput {
        id: 1,
        name: "Ops".to_owned(),
        priority: 5,
        note: Some("has a note".to_owned()),
    };
    assert_eq!(built, literal);
}

#[test]
fn create_input_builder_matches_struct_literal_with_optional_field_omitted() {
    // `.note(..)` never called — the optional setter's own `Default`
    // (`None`) must match a struct literal that spells it out.
    let built = CreateBuilderWidgetInput::builder()
        .id(2)
        .name("Ops2")
        .priority(1)
        .build();
    let literal = CreateBuilderWidgetInput {
        id: 2,
        name: "Ops2".to_owned(),
        priority: 1,
        note: None,
    };
    assert_eq!(built, literal);
}

// ───── #2 Update input: all three double-option patch states ────────────

#[test]
fn update_input_builder_untouched_state_matches_default() {
    // No setters called at all — every field (all optional on an update
    // input) must come out exactly `Default::default()`.
    let built = UpdateBuilderWidgetInput::builder().build();
    let literal = UpdateBuilderWidgetInput::default();
    assert_eq!(built, literal);
    assert_eq!(
        built.note, None,
        "untouched nullable field must be outer None"
    );
}

#[test]
fn update_input_builder_set_state_matches_struct_literal() {
    let built = UpdateBuilderWidgetInput::builder()
        .name("Renamed")
        .priority(9)
        .note(Some("now set".to_owned()))
        .build();
    let literal = UpdateBuilderWidgetInput {
        name: Some("Renamed".to_owned()),
        priority: Some(9),
        note: Some(Some("now set".to_owned())),
    };
    assert_eq!(built, literal);
}

#[test]
fn update_input_builder_explicit_clear_state_matches_struct_literal() {
    // cratestack#567: calling `.note(None)` is "the caller touched this
    // nullable field and asked to clear it" — `Some(None)`, not the
    // untouched `None`.
    let built = UpdateBuilderWidgetInput::builder().note(None).build();
    let literal = UpdateBuilderWidgetInput {
        name: None,
        priority: None,
        note: Some(None),
    };
    assert_eq!(built, literal);
    assert_eq!(
        built.note,
        Some(None),
        "an explicit clear must be Some(None), not the untouched outer None"
    );
}

// ───── #3 Serde round-trip: builder output serializes identically ───────

#[test]
fn update_input_builder_set_state_serializes_identically_to_the_struct_literal() {
    let built = UpdateBuilderWidgetInput::builder()
        .name("Renamed")
        .priority(9)
        .note(Some("now set".to_owned()))
        .build();
    let literal = UpdateBuilderWidgetInput {
        name: Some("Renamed".to_owned()),
        priority: Some(9),
        note: Some(Some("now set".to_owned())),
    };
    assert_eq!(
        serde_json::to_string(&built).expect("serialize builder-built value"),
        serde_json::to_string(&literal).expect("serialize struct literal"),
    );
}

#[test]
fn update_input_builder_clear_state_serializes_identically_to_the_struct_literal() {
    let built = UpdateBuilderWidgetInput::builder().note(None).build();
    let literal = UpdateBuilderWidgetInput {
        name: None,
        priority: None,
        note: Some(None),
    };
    let built_json = serde_json::to_string(&built).expect("serialize builder-built value");
    let literal_json = serde_json::to_string(&literal).expect("serialize struct literal");
    assert_eq!(built_json, literal_json);
    assert!(
        built_json.contains("\"note\":null"),
        "an explicit clear must still serialize `note` as null, not omit it: {built_json}"
    );
}

#[test]
fn update_input_builder_untouched_fields_are_omitted_from_the_wire() {
    // Both arities now carry `skip_serializing_if` — see
    // `struct_only.rs::struct_field_definition`'s serde-attr match. Until
    // cratestack#663 only the *nullable* field (`note`) did; a non-nullable
    // patch field (`priority`) had no `Required` arm and so serialized as
    // `null` when untouched, which the server could not distinguish from
    // an explicit write. That is the bug #663 fixed, so this test now
    // asserts the opposite of what it originally did for `priority`:
    // untouched means absent, for *every* arity.
    //
    // `note` (nullable) keeps its third state: an explicit clear still
    // sends `"note":null`, asserted by
    // `update_input_builder_explicit_clear_state_matches_struct_literal`
    // above. Omission here means untouched, not cleared.
    let built = UpdateBuilderWidgetInput::builder().name("Renamed").build();
    let literal = UpdateBuilderWidgetInput {
        name: Some("Renamed".to_owned()),
        priority: None,
        note: None,
    };
    let built_json = serde_json::to_string(&built).expect("serialize builder-built value");
    let literal_json = serde_json::to_string(&literal).expect("serialize struct literal");
    assert_eq!(built_json, literal_json);
    assert!(
        !built_json.contains("note"),
        "an untouched nullable field must be omitted from the wire, not sent as null: {built_json}"
    );
    assert!(
        !built_json.contains("priority"),
        "cratestack#663: an untouched non-nullable patch field must also be omitted, not sent \
         as null — a null here is indistinguishable from an explicit write and clobbers the \
         column server-side: {built_json}"
    );
    assert_eq!(
        built_json, r#"{"name":"Renamed"}"#,
        "only the field the caller actually touched may reach the wire: {built_json}"
    );
}

// ───── #4 model struct + view struct builders ────────────────────────────

#[test]
fn model_struct_builder_matches_struct_literal() {
    let built = BuilderWidget::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .note(Some("has a note".to_owned()))
        .build();
    let literal = BuilderWidget {
        id: 1,
        name: "Ops".to_owned(),
        priority: 5,
        note: Some("has a note".to_owned()),
    };
    assert_eq!(built, literal);
}

#[test]
fn view_struct_builder_matches_struct_literal() {
    let built = BuilderWidgetSummary::builder()
        .id(1)
        .name("Ops")
        .note(Some("has a note".to_owned()))
        .build();
    let literal = BuilderWidgetSummary {
        id: 1,
        name: "Ops".to_owned(),
        note: Some("has a note".to_owned()),
    };
    assert_eq!(built, literal);
}
