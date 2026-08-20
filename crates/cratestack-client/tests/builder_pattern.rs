//! Positive proof that the typestate builder every struct-shaped generated
//! type gets (`cratestack-core/src/builder.rs`, `cratestack-macros/src/
//! builder.rs`) produces output byte-for-byte equal to the equivalent
//! struct literal, for the two query-side types `include_embedded_schema!`
//! doesn't generate at all (`{Model}Where` / `{Model}FindManyInput` — see
//! `crates/cratestack-macros/src/include/client.rs` vs. `embedded.rs`,
//! only the former wires `generate_find_many_types`). Companion to
//! `crates/cratestack-sqlite/tests/builder_pattern.rs`, which covers the
//! model struct, view struct, and `Create`/`Update` inputs.
//!
//! Fully DB-free like this crate's other tests (see `generated_client.rs`'s
//! module doc): `include_client_schema!` needs no live database, and
//! nothing here even spins up the mock HTTP server — every assertion is a
//! plain struct comparison.
//!
//! `BuilderWidget.tags` (`String[]`) additionally covers the
//! `Create`/`Update{Model}Input` half of the list-arity append setter
//! (cratestack#661) that `crates/cratestack-sqlite/tests/builder_pattern.rs`
//! *can't* — a `datasource`-bound model (sqlite's fixture) rejects a
//! scalar list field outright (no SQL bind representation), so the
//! `Update{Model}Input` "append implies touched" / "untouched stays off
//! the wire" shapes can only be exercised on a schema reachable solely
//! through `include_client_schema!`, which is exactly this fixture — see
//! its own doc comment for the full explanation.
//!
//! `tagWidgets`'s `Args.tags` covers the third, previously-missing half:
//! `procedure_arg_builder_fields` is its own code path (not
//! `model_builder_fields`/`scoped_builder_fields`), and used to skip
//! `.with_list(..)` entirely — Dart's equivalent `TagWidgetsArgsBuilder`
//! got `.addTags(..)` for free (it goes through the ordinary
//! `build_data_class`) while the Rust `Args::builder()` had no
//! `.add_tags(..)` at all.

mod builder_schema {
    cratestack::include_client_schema!("tests/fixtures/builder_pattern.cstack");
}

use builder_schema::cratestack_schema::{
    BuilderWidgetFindManyInput, BuilderWidgetOrderByClause, BuilderWidgetSortField,
    BuilderWidgetWhere,
};
use cratestack::{FieldFilterInput, SortDirection};

// ───── #1 `{Model}Where`: every field optional, non-generic builder ─────

#[test]
fn where_builder_matches_struct_literal() {
    let built = BuilderWidgetWhere::builder()
        .name(Some(FieldFilterInput {
            contains: Some("wid".to_owned()),
            ..Default::default()
        }))
        .priority(Some(FieldFilterInput {
            gte: Some(5),
            ..Default::default()
        }))
        .build();
    let literal = BuilderWidgetWhere {
        name: Some(FieldFilterInput {
            contains: Some("wid".to_owned()),
            ..Default::default()
        }),
        priority: Some(FieldFilterInput {
            gte: Some(5),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(built, literal);
}

#[test]
fn where_builder_with_no_setters_matches_default() {
    let built = BuilderWidgetWhere::builder().build();
    assert_eq!(built, BuilderWidgetWhere::default());
}

// ───── #2 `{Model}FindManyInput`: composes `Where` + `OrderByClause` ─────

#[test]
fn find_many_input_builder_matches_struct_literal() {
    let built = BuilderWidgetFindManyInput::builder()
        .r#where(Some(BuilderWidgetWhere {
            id: Some(FieldFilterInput {
                eq: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        }))
        .order_by(Some(vec![BuilderWidgetOrderByClause {
            field: BuilderWidgetSortField::Priority,
            direction: SortDirection::Desc,
        }]))
        .build();
    let literal = BuilderWidgetFindManyInput {
        r#where: Some(BuilderWidgetWhere {
            id: Some(FieldFilterInput {
                eq: Some(1),
                ..Default::default()
            }),
            ..Default::default()
        }),
        order_by: Some(vec![BuilderWidgetOrderByClause {
            field: BuilderWidgetSortField::Priority,
            direction: SortDirection::Desc,
        }]),
    };
    assert_eq!(built, literal);
}

#[test]
fn find_many_input_builder_with_no_setters_matches_default() {
    let built = BuilderWidgetFindManyInput::builder().build();
    assert_eq!(built, BuilderWidgetFindManyInput::default());
}

// ───── #3 `{Model}OrderByClause`: both fields required ───────────────────

#[test]
fn order_by_clause_builder_matches_struct_literal() {
    let built = BuilderWidgetOrderByClause::builder()
        .field(BuilderWidgetSortField::Name)
        .direction(SortDirection::Asc)
        .build();
    let literal = BuilderWidgetOrderByClause {
        field: BuilderWidgetSortField::Name,
        direction: SortDirection::Asc,
    };
    assert_eq!(built, literal);
}
