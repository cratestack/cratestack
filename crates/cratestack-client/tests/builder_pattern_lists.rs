//! The list-arity half of the builder guarantee (cratestack#661), split from
//! the sibling `builder_pattern.rs` per the repo's ~200-LoC file ceiling.
//!
//! These shapes can only be exercised on a schema reachable solely through
//! `include_client_schema!`: a `datasource`-bound model rejects a scalar list
//! field outright (there is no SQL bind representation for one), so the
//! sqlite fixture cannot carry them. Covers all three builder paths that
//! generate list setters — `Create`/`Update{Model}Input` via
//! `model_builder_fields`, and procedure `Args` via the separate
//! `procedure_arg_builder_fields`, which is its own code path and originally
//! skipped `.with_list(..)` entirely.

mod builder_schema {
    cratestack::include_client_schema!("tests/fixtures/builder_pattern.cstack");
}

use builder_schema::cratestack_schema::{
    CreateBuilderWidgetInput, UpdateBuilderWidgetInput, procedures::tag_widgets,
};

// ───── #4 `Create{Model}Input.tags`: non-patch list append (cratestack#661) ─

#[test]
fn create_input_unset_list_field_builds_as_empty_vec() {
    let built = CreateBuilderWidgetInput::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .build();
    assert_eq!(built.tags, Vec::<String>::new());
}

#[test]
fn create_input_append_setter_preserves_call_order() {
    let built = CreateBuilderWidgetInput::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .add_tags("rust")
        .add_tags("codegen")
        .build();
    assert_eq!(built.tags, vec!["rust".to_owned(), "codegen".to_owned()]);
}

#[test]
fn create_input_bulk_setter_after_append_replaces_appended_items() {
    let built = CreateBuilderWidgetInput::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .add_tags("rust")
        .tags(vec!["only".to_owned()])
        .build();
    assert_eq!(built.tags, vec!["only".to_owned()]);
}

#[test]
fn create_input_append_after_bulk_setter_appends_to_the_bulk_value() {
    let built = CreateBuilderWidgetInput::builder()
        .id(1)
        .name("Ops")
        .priority(5)
        .tags(vec!["seed".to_owned()])
        .add_tags("extra")
        .build();
    assert_eq!(built.tags, vec!["seed".to_owned(), "extra".to_owned()]);
}

// ───── #5 `Update{Model}Input.tags`: patch "touched" semantics (cratestack#661) ─

#[test]
fn update_input_untouched_list_field_stays_default_and_off_the_wire() {
    // No setter, no append: the untouched patch field must come out as
    // the outer `None` ("caller never mentioned this field") the same as
    // every other untouched `Update{Model}Input` field, and must be
    // omitted from the wire entirely — not sent as `null` the way an
    // untouched non-nullable scalar patch field (`priority`) legitimately
    // is (see `update_input_builder_untouched_nullable_field_omits_it_on_the_wire`
    // in the sqlite companion file for that contrast).
    let built = UpdateBuilderWidgetInput::builder().name("Renamed").build();
    assert_eq!(built.tags, None, "untouched list field must be outer None");
    let built_json = serde_json::to_string(&built).expect("serialize builder-built value");
    assert!(
        !built_json.contains("tags"),
        "an untouched list field must be omitted from the wire, not sent as null: {built_json}"
    );
}

#[test]
fn update_input_append_marks_the_field_touched() {
    // `.add_tags(x)` on a field nobody set makes the patch `Some(vec![x])`
    // — appending IS touching, even though no bulk `.tags(..)` setter was
    // ever called.
    let built = UpdateBuilderWidgetInput::builder().add_tags("rust").build();
    assert_eq!(built.tags, Some(vec!["rust".to_owned()]));
}

#[test]
fn update_input_append_preserves_call_order_and_serializes_the_touched_value() {
    let built = UpdateBuilderWidgetInput::builder()
        .add_tags("rust")
        .add_tags("codegen")
        .build();
    assert_eq!(
        built.tags,
        Some(vec!["rust".to_owned(), "codegen".to_owned()])
    );
    let built_json = serde_json::to_string(&built).expect("serialize builder-built value");
    assert!(
        built_json.contains(r#""tags":["rust","codegen"]"#),
        "a touched list field must serialize its full value: {built_json}"
    );
}

#[test]
fn update_input_append_after_bulk_setter_appends_to_the_bulk_value() {
    let built = UpdateBuilderWidgetInput::builder()
        .tags(vec!["seed".to_owned()])
        .add_tags("extra")
        .build();
    assert_eq!(
        built.tags,
        Some(vec!["seed".to_owned(), "extra".to_owned()])
    );
}

#[test]
fn update_input_bulk_setter_after_append_replaces_appended_items() {
    let built = UpdateBuilderWidgetInput::builder()
        .add_tags("rust")
        .tags(vec!["only".to_owned()])
        .build();
    assert_eq!(built.tags, Some(vec!["only".to_owned()]));
}

// ───── #6 `tagWidgets::Args.tags`: procedure-arg append setter (cratestack#661) ─

#[test]
fn procedure_args_unset_list_field_builds_as_empty_vec() {
    let built = tag_widgets::Args::builder().build();
    assert_eq!(built.tags, Vec::<String>::new());
}

#[test]
fn procedure_args_append_setter_preserves_call_order() {
    let built = tag_widgets::Args::builder()
        .add_tags("rust")
        .add_tags("codegen")
        .build();
    assert_eq!(built.tags, vec!["rust".to_owned(), "codegen".to_owned()]);
}

#[test]
fn procedure_args_bulk_setter_after_append_replaces_appended_items() {
    let built = tag_widgets::Args::builder()
        .add_tags("rust")
        .tags(vec!["only".to_owned()])
        .build();
    assert_eq!(built.tags, vec!["only".to_owned()]);
}

#[test]
fn procedure_args_append_after_bulk_setter_appends_to_the_bulk_value() {
    let built = tag_widgets::Args::builder()
        .tags(vec!["seed".to_owned()])
        .add_tags("extra")
        .build();
    assert_eq!(built.tags, vec!["seed".to_owned(), "extra".to_owned()]);
}
