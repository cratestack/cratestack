//! Index diff for one (prev, next) table pair.

use std::collections::BTreeSet;

use crate::convert::TableProjection;
use crate::ir::{DropIndex, Op};

#[derive(Default)]
pub(super) struct IndexOps {
    pub adds: Vec<Op>,
    pub drops: Vec<Op>,
}

pub(super) fn diff_indexes(prev: &TableProjection, next: &TableProjection) -> IndexOps {
    let mut out = IndexOps::default();

    let prev_names: BTreeSet<&str> = prev.indexes.iter().map(|i| i.name.as_str()).collect();
    let next_names: BTreeSet<&str> = next.indexes.iter().map(|i| i.name.as_str()).collect();

    for index in &prev.indexes {
        if !next_names.contains(index.name.as_str()) {
            out.drops.push(Op::DropIndex(DropIndex {
                name: index.name.clone(),
                table: index.table.clone(),
            }));
        }
    }
    for index in &next.indexes {
        if !prev_names.contains(index.name.as_str()) {
            out.adds.push(Op::AddIndex(index.clone()));
        }
    }

    out
}
