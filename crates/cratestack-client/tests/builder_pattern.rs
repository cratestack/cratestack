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
