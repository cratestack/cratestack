use super::*;
use crate::ir::{
    AddCheck, AddColumn, AlterColumnNullability, CheckKind, Column, ColumnDefault, ColumnType,
    DropColumn,
};

fn column(name: &str, arity: ColumnArity, default: Option<ColumnDefault>) -> Column {
    Column {
        name: name.to_owned(),
        ty: ColumnType::Scalar("Int".to_owned()),
        arity,
        default,
        primary_key: false,
    }
}

fn promote_to_not_null(table: &str, column: &str) -> Op {
    Op::AlterColumnNullability(AlterColumnNullability {
        table: table.to_owned(),
        column: column.to_owned(),
        from: ColumnArity::Optional,
        to: ColumnArity::Required,
    })
}

#[test]
fn no_blocking_ops_yields_no_reasons() {
    let ops = vec![Op::DropColumn(DropColumn {
        table: "products".to_owned(),
        column: "legacy".to_owned(),
    })];
    assert_eq!(blocking_reasons(&ops), Vec::new());
}

/// The exact shape from cratestack#843: `version Int?` → `version Int`.
/// The old warning called this "a required column was added without a
/// default", which is a different operation entirely — no column is
/// added here.
#[test]
fn not_null_promotion_names_the_column_and_offers_a_backfill() {
    let reasons = blocking_reasons(&[promote_to_not_null("products", "version")]);

    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].target(), "products.version");
    assert!(
        reasons[0].cause.contains("NOT NULL"),
        "cause was: {}",
        reasons[0].cause
    );
    assert_eq!(
        reasons[0].remedy.as_deref(),
        Some("UPDATE products SET version = <value> WHERE version IS NULL;")
    );
}

/// A column that does not exist yet cannot be backfilled by a
/// pre-script, so this case must *not* hand the operator a remedy
/// template that would fail — the whole class of bug #843 is about
/// naming a remedy that does not work.
#[test]
fn added_required_column_without_default_offers_no_backfill_template() {
    let reasons = blocking_reasons(&[Op::AddColumn(AddColumn {
        table: "orders".to_owned(),
        column: column("total", ColumnArity::Required, None),
    })]);

    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].target(), "orders.total");
    assert_eq!(reasons[0].remedy, None);
}

#[test]
fn added_required_column_with_a_default_is_not_blocking() {
    let reasons = blocking_reasons(&[Op::AddColumn(AddColumn {
        table: "orders".to_owned(),
        column: column(
            "total",
            ColumnArity::Required,
            Some(ColumnDefault::Literal("0".to_owned())),
        ),
    })]);
    assert_eq!(reasons, Vec::new());
}

#[test]
fn added_optional_column_is_not_blocking() {
    let reasons = blocking_reasons(&[Op::AddColumn(AddColumn {
        table: "orders".to_owned(),
        column: column("note", ColumnArity::Optional, None),
    })]);
    assert_eq!(reasons, Vec::new());
}

#[test]
fn validator_check_blocks_and_names_the_constraint() {
    let reasons = blocking_reasons(&[Op::AddCheck(AddCheck {
        table: "orders".to_owned(),
        column: "total".to_owned(),
        name: "orders_total_range_check".to_owned(),
        kind: CheckKind::Range {
            min: Some(0),
            max: None,
        },
    })]);

    assert_eq!(reasons.len(), 1);
    assert!(
        reasons[0].cause.contains("orders_total_range_check"),
        "cause was: {}",
        reasons[0].cause
    );
}

/// Enum-membership checks are classified `Safe` by
/// `Op::destructiveness`; `blocking_reasons` must agree with that
/// classification rather than re-deriving it.
#[test]
fn enum_membership_check_is_not_blocking() {
    let reasons = blocking_reasons(&[Op::AddCheck(AddCheck {
        table: "orders".to_owned(),
        column: "status".to_owned(),
        name: "orders_status_enum_check".to_owned(),
        kind: CheckKind::Enum {
            variants: vec!["pending".to_owned(), "shipped".to_owned()],
            list: false,
        },
    })]);
    assert_eq!(reasons, Vec::new());
}

/// `blocking_reasons` is used in place of the old `has_blocking` bit,
/// so "non-empty" and "some op is Blocking" must not be able to
/// disagree.
#[test]
fn non_empty_exactly_when_some_op_is_blocking() {
    let ops = vec![
        Op::DropColumn(DropColumn {
            table: "products".to_owned(),
            column: "legacy".to_owned(),
        }),
        promote_to_not_null("products", "version"),
    ];

    let any_blocking = ops.iter().any(|op| op.destructiveness().is_blocking());
    assert_eq!(!blocking_reasons(&ops).is_empty(), any_blocking);
}

#[test]
fn reasons_are_reported_in_emission_order() {
    let ops = vec![
        promote_to_not_null("a", "one"),
        promote_to_not_null("b", "two"),
    ];
    let targets: Vec<_> = blocking_reasons(&ops)
        .iter()
        .map(BlockingReason::target)
        .collect();
    assert_eq!(targets, vec!["a.one", "b.two"]);
}
