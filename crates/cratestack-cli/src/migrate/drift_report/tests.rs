use cratestack_migrate::ir::{AddCheck, CheckKind, CreateTable, Op};

use super::render;

fn range_check(table: &str) -> Op {
    Op::AddCheck(AddCheck {
        table: table.to_owned(),
        column: "total".to_owned(),
        name: format!("{table}_total_range_check"),
        kind: CheckKind::Range {
            min: Some(0),
            max: None,
        },
    })
}

/// Baseline's diff is `(live database, schema)`. A table the schema
/// declares but the database lacks gets `CreateTable` plus its CHECK,
/// which is not blocking (cratestack#1129).
#[test]
fn check_on_table_the_schema_creates_is_not_reported_blocking() {
    let ops = vec![
        Op::CreateTable(CreateTable {
            name: "orders".to_owned(),
            columns: Vec::new(),
        }),
        range_check("orders"),
    ];
    let out = render(&ops, &[]);
    assert!(!out.contains("[blocking]"), "report was:\n{out}");
    assert!(out.contains("[safe]"), "report was:\n{out}");
}

#[test]
fn check_on_existing_table_is_still_reported_blocking() {
    let out = render(&[range_check("orders")], &[]);
    assert!(out.contains("[blocking]"), "report was:\n{out}");
}
