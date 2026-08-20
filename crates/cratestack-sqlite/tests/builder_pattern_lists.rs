//! The list-arity half of the builder guarantee (cratestack#661) for the
//! embedded path, split from the sibling `builder_pattern.rs` per the repo's
//! ~200-LoC file ceiling.
//!
//! Scoped to what a `datasource`-bound schema can actually express: a scalar
//! list field is rejected outright on a database-backed model, so the list
//! setters here are exercised through the schema's `type` block rather than
//! a model. The `Create`/`Update{Model}Input` list shapes live in
//! `crates/cratestack-client/tests/builder_pattern_lists.rs` instead.

use cratestack::include_embedded_schema;

include_embedded_schema!("tests/fixtures/builder_pattern.cstack");

use cratestack_schema::BuilderWidgetTags;

// ───── #5 `BuilderWidgetTags`: the list-arity append setter (cratestack#661) ─

#[test]
fn unset_list_field_builds_as_empty_vec() {
    // `is_required` treats `Vec<T>` as optional the same way `Option<T>`
    // is — an unset list must build as `[]`, not panic/error.
    let built = BuilderWidgetTags::builder().build();
    assert_eq!(built, BuilderWidgetTags { tags: Vec::new() });
}

#[test]
fn append_setter_preserves_call_order() {
    let built = BuilderWidgetTags::builder()
        .add_tags("rust")
        .add_tags("codegen")
        .add_tags("builder")
        .build();
    assert_eq!(
        built,
        BuilderWidgetTags {
            tags: vec![
                "rust".to_owned(),
                "codegen".to_owned(),
                "builder".to_owned()
            ],
        }
    );
}

#[test]
fn bulk_setter_after_append_replaces_appended_items() {
    // The bulk setter still *replaces* — appending first must not leak
    // into a later bulk call.
    let built = BuilderWidgetTags::builder()
        .add_tags("rust")
        .add_tags("codegen")
        .tags(vec!["only".to_owned(), "these".to_owned()])
        .build();
    assert_eq!(
        built,
        BuilderWidgetTags {
            tags: vec!["only".to_owned(), "these".to_owned()],
        }
    );
}

#[test]
fn append_after_bulk_setter_appends_to_the_bulk_value() {
    let built = BuilderWidgetTags::builder()
        .tags(vec!["seed".to_owned()])
        .add_tags("extra")
        .build();
    assert_eq!(
        built,
        BuilderWidgetTags {
            tags: vec!["seed".to_owned(), "extra".to_owned()],
        }
    );
}
