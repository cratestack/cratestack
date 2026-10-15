//! A CHECK on a table this migration creates is not blocking
//! (cratestack#1129): the table has no rows yet, so nothing can fail
//! the CHECK. Kept apart from `tests.rs` for the 200-line budget.

use super::*;
use crate::ir::{AddCheck, CheckKind, CreateTable};

fn create_table(name: &str) -> Op {
    Op::CreateTable(CreateTable {
        name: name.to_owned(),
        columns: Vec::new(),
    })
}

fn range_check(table: &str, column: &str) -> Op {
    Op::AddCheck(AddCheck {
        table: table.to_owned(),
        column: column.to_owned(),
        name: format!("{table}_{column}_range_check"),
        kind: CheckKind::Range {
            min: Some(0),
            max: None,
        },
    })
}

/// The first migration of a schema with `@range @db_enforce` emits
/// `CreateTable` and the CHECK together. Neither can block.
#[test]
fn validator_check_on_table_created_in_same_migration_is_not_blocking() {
    let ops = vec![create_table("orders"), range_check("orders", "total")];
    assert_eq!(blocking_reasons(&ops), Vec::new());
}

/// The context-aware class exempts only the CHECK on the created table;
/// the same CHECK on an unrelated table keeps its per-op Blocking class.
#[test]
fn destructiveness_in_context_exempts_only_checks_on_created_tables() {
    let ops = vec![
        create_table("orders"),
        range_check("orders", "total"),
        range_check("widgets", "size"),
    ];
    assert_eq!(
        destructiveness_in_context(&ops),
        vec![
            Destructiveness::Safe,
            Destructiveness::Safe,
            Destructiveness::Blocking,
        ]
    );
}

/// The exemption is keyed on a `CreateTable` of the same table. A CHECK
/// on a table that already exists stays blocking even when the same
/// migration also creates an unrelated table.
#[test]
fn validator_check_on_existing_table_stays_blocking_beside_an_unrelated_create() {
    let ops = vec![create_table("widgets"), range_check("orders", "total")];
    let reasons = blocking_reasons(&ops);
    assert_eq!(reasons.len(), 1);
    assert_eq!(reasons[0].target(), "orders.total");
}
