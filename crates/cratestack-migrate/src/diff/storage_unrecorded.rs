//! `@@map` / `@map` against a snapshot that does not say what each table and
//! column stores (cratestack#1375).
//!
//! `migrate baseline` snapshots the introspected database, whose tables carry
//! no `model` and no `field_columns` (the baseline stamps them where it can,
//! `crate::storage_stamp`; a snapshot from before the fields existed has none
//! either). Against such a table `super::storage_names` cannot compare, so a
//! changed `@@map` would reach the differ as a vanished table and a new one.
//! Here the cases it cannot decide are refused: a mapped model with no
//! previous table while a table of unknown provenance vanished, and a mapped
//! field whose column is new while a column of an unrecorded table vanished.
//! A rename marker says what it is, and a new unmapped model is untouched.

use std::collections::BTreeSet;

use crate::convert::TableProjection;
use crate::error::MigrateError;
use crate::naming::{column_name, table_name};
use crate::projection::Projections;

/// A mapped model whose table the previous side lacks, while a table it holds
/// with no recorded model is gone.
pub(super) fn check_new_table(
    prev: &Projections,
    next: &Projections,
    table: &TableProjection,
    model: &str,
) -> Result<(), MigrateError> {
    if table.rename_from.is_some() || table.name == table_name(model) {
        return Ok(());
    }
    let renamed_from: BTreeSet<&str> = next
        .tables
        .values()
        .filter_map(|t| t.rename_from.as_deref())
        .collect();
    let gone: Vec<&str> = prev
        .tables
        .values()
        .filter(|was| was.model.is_none())
        .map(|was| was.name.as_str())
        .filter(|name| !next.tables.contains_key(*name) && !renamed_from.contains(name))
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    Err(MigrateError::TableStorageNameUnrecorded {
        model: model.to_owned(),
        to: table.name.clone(),
        candidates: quoted(&gone),
        renames: renames("@@rename", &gone),
    })
}

/// A mapped field in a column the previous table lacks, while a column of a
/// table with no recorded fields is gone.
pub(super) fn check_columns(
    was: &TableProjection,
    table: &TableProjection,
    model: &str,
) -> Result<(), MigrateError> {
    if !was.field_columns.is_empty() {
        return Ok(());
    }
    let now: BTreeSet<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
    let renamed_from: BTreeSet<&str> = table
        .column_renames
        .iter()
        .map(|(_, old)| old.as_str())
        .collect();
    let gone: Vec<&str> = was
        .columns
        .iter()
        .map(|c| c.name.as_str())
        .filter(|name| !now.contains(name) && !renamed_from.contains(name))
        .collect();
    if gone.is_empty() {
        return Ok(());
    }
    for (field, column) in &table.field_columns {
        let renamed = table.column_renames.iter().any(|(new, _)| new == column);
        let known = was.columns.iter().any(|c| c.name == *column);
        if *column != column_name(field) && !known && !renamed {
            return Err(MigrateError::ColumnStorageNameUnrecorded {
                table: table.name.clone(),
                model: model.to_owned(),
                field: field.clone(),
                to: column.clone(),
                candidates: quoted(&gone),
                renames: renames("@rename", &gone),
            });
        }
    }
    Ok(())
}

fn quoted(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The marker to write, one per candidate: `@@rename(from = "legacy")`.
fn renames(marker: &str, names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("`{marker}(from = \"{name}\")`"))
        .collect::<Vec<_>>()
        .join(" or ")
}
