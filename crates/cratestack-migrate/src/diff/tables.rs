//! Table-level diff phase: resolve rename markers, then collect the
//! tables that appear / disappear.

use std::collections::{BTreeMap, BTreeSet};

use crate::convert::TableProjection;
use crate::ir::{CreateTable, DropTable, Op, RenameTable};

/// Result of resolving the table-rename markers across the two
/// schemas. `renamed_from` maps the new (next-side) table name to
/// the old (prev-side) name that the rename consumes.
pub(super) struct RenameMap<'a> {
    pub renames: Vec<Op>,
    pub renamed_from: BTreeMap<&'a str, &'a str>,
}

pub(super) fn resolve_renames<'a>(
    prev_tables: &'a BTreeMap<String, TableProjection>,
    next_tables: &'a BTreeMap<String, TableProjection>,
) -> RenameMap<'a> {
    let mut renames = Vec::new();
    let mut renamed_from: BTreeMap<&str, &str> = BTreeMap::new();
    for (new_name, projection) in next_tables {
        let Some(old_name) = projection.rename_from.as_deref() else {
            continue;
        };
        if !prev_tables.contains_key(old_name) {
            continue;
        }
        if prev_tables.contains_key(new_name.as_str()) {
            // The new name already exists in prev — this is not a
            // rename, it's a collision. Fall through to drop+add.
            continue;
        }
        renames.push(Op::RenameTable(RenameTable {
            from: old_name.to_owned(),
            to: new_name.clone(),
        }));
        renamed_from.insert(new_name.as_str(), old_name);
    }
    RenameMap {
        renames,
        renamed_from,
    }
}

pub(super) fn collect_drops(
    prev_tables: &BTreeMap<String, TableProjection>,
    next_tables: &BTreeMap<String, TableProjection>,
    renamed_from: &BTreeMap<&str, &str>,
) -> Vec<Op> {
    let consumed_old: BTreeSet<&str> = renamed_from.values().copied().collect();
    let mut drops = Vec::new();
    for name in prev_tables.keys() {
        if consumed_old.contains(name.as_str()) {
            continue;
        }
        if !next_tables.contains_key(name) {
            drops.push(Op::DropTable(DropTable { name: name.clone() }));
        }
    }
    drops
}

/// Returns `(create_tables, add_indexes_for_new_tables, add_checks_for_new_tables)`.
pub(super) fn collect_creates(
    prev_tables: &BTreeMap<String, TableProjection>,
    next_tables: &BTreeMap<String, TableProjection>,
    renamed_from: &BTreeMap<&str, &str>,
) -> (Vec<Op>, Vec<Op>, Vec<Op>) {
    let mut create_tables = Vec::new();
    let mut add_indexes = Vec::new();
    let mut add_checks = Vec::new();
    for (name, projection) in next_tables {
        if renamed_from.contains_key(name.as_str()) {
            continue;
        }
        if !prev_tables.contains_key(name) {
            create_tables.push(Op::CreateTable(CreateTable {
                name: name.clone(),
                columns: projection.columns.clone(),
            }));
            for index in &projection.indexes {
                add_indexes.push(Op::AddIndex(index.clone()));
            }
            for check in &projection.checks {
                add_checks.push(Op::AddCheck(check.clone()));
            }
        }
    }
    (create_tables, add_indexes, add_checks)
}
