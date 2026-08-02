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
    let mut dropped: BTreeSet<&str> = BTreeSet::new();
    for name in prev_tables.keys() {
        if consumed_old.contains(name.as_str()) {
            continue;
        }
        if !next_tables.contains_key(name) {
            dropped.insert(name.as_str());
        }
    }
    topo_sort_drops(prev_tables, dropped)
        .into_iter()
        .map(|name| {
            Op::DropTable(DropTable {
                name: name.to_owned(),
            })
        })
        .collect()
}

/// Orders a set of tables being dropped so that a table with a
/// foreign key to another table in the same drop set drops first.
/// Postgres refuses `DROP TABLE parent` while `child`'s FK constraint
/// still references it, so alphabetical (the `BTreeSet` default)
/// isn't safe once relations carry real constraints (issue #260).
///
/// A cycle between two tables in the drop set (mutual FKs) can't be
/// satisfied by any order — the remainder is emitted in its original
/// order rather than looping forever; a real cycle needs `CASCADE` or
/// a hand-written migration, which is out of scope here.
fn topo_sort_drops<'a>(
    prev_tables: &'a BTreeMap<String, TableProjection>,
    names: BTreeSet<&'a str>,
) -> Vec<&'a str> {
    let mut in_degree: BTreeMap<&str, usize> = names.iter().map(|name| (*name, 0)).collect();
    let mut edges: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for &name in &names {
        let Some(projection) = prev_tables.get(name) else {
            continue;
        };
        for fk in &projection.foreign_keys {
            let target = fk.referenced_table.as_str();
            if target != name && names.contains(target) {
                edges.entry(name).or_default().push(target);
                *in_degree.get_mut(target).expect("target is in names") += 1;
            }
        }
    }

    let mut ready: BTreeSet<&str> = in_degree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(name, _)| *name)
        .collect();
    let mut result = Vec::with_capacity(names.len());
    while let Some(&node) = ready.iter().next() {
        ready.remove(node);
        result.push(node);
        for &target in edges.get(node).into_iter().flatten() {
            let degree = in_degree.get_mut(target).expect("target is in names");
            *degree -= 1;
            if *degree == 0 {
                ready.insert(target);
            }
        }
    }
    if result.len() < names.len() {
        for &name in &names {
            if !result.contains(&name) {
                result.push(name);
            }
        }
    }
    result
}

/// Returns `(create_tables, add_indexes, add_checks, add_foreign_keys)`
/// for every newly created table.
pub(super) fn collect_creates(
    prev_tables: &BTreeMap<String, TableProjection>,
    next_tables: &BTreeMap<String, TableProjection>,
    renamed_from: &BTreeMap<&str, &str>,
) -> (Vec<Op>, Vec<Op>, Vec<Op>, Vec<Op>) {
    let mut create_tables = Vec::new();
    let mut add_indexes = Vec::new();
    let mut add_checks = Vec::new();
    let mut add_foreign_keys = Vec::new();
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
            for fk in &projection.foreign_keys {
                add_foreign_keys.push(Op::AddForeignKey(fk.clone()));
            }
        }
    }
    (create_tables, add_indexes, add_checks, add_foreign_keys)
}
