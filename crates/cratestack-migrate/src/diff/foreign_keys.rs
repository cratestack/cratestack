//! Foreign-key diff for one (prev, next) table pair.

use std::collections::BTreeMap;

use crate::convert::TableProjection;
use crate::ir::{AddForeignKey, DropForeignKey, Op};

#[derive(Default)]
pub(super) struct ForeignKeyOps {
    pub adds: Vec<Op>,
    pub drops: Vec<Op>,
}

pub(super) fn diff_foreign_keys(prev: &TableProjection, next: &TableProjection) -> ForeignKeyOps {
    let mut out = ForeignKeyOps::default();

    let prev_by_name: BTreeMap<&str, &AddForeignKey> = prev
        .foreign_keys
        .iter()
        .map(|fk| (fk.name.as_str(), fk))
        .collect();
    let next_by_name: BTreeMap<&str, &AddForeignKey> = next
        .foreign_keys
        .iter()
        .map(|fk| (fk.name.as_str(), fk))
        .collect();

    for fk in &prev.foreign_keys {
        if !next_by_name.contains_key(fk.name.as_str()) {
            out.drops.push(Op::DropForeignKey(to_drop(fk)));
        }
    }
    for fk in &next.foreign_keys {
        match prev_by_name.get(fk.name.as_str()) {
            None => out.adds.push(Op::AddForeignKey(fk.clone())),
            // Same name (same table + column), but the target changed
            // — the relation now points somewhere else. Drop the old
            // constraint and add the new one.
            Some(prev_fk) if *prev_fk != fk => {
                out.drops.push(Op::DropForeignKey(to_drop(prev_fk)));
                out.adds.push(Op::AddForeignKey(fk.clone()));
            }
            _ => {}
        }
    }

    out
}

fn to_drop(fk: &AddForeignKey) -> DropForeignKey {
    DropForeignKey {
        name: fk.name.clone(),
        table: fk.table.clone(),
        column: fk.column.clone(),
        referenced_table: fk.referenced_table.clone(),
        referenced_column: fk.referenced_column.clone(),
    }
}
