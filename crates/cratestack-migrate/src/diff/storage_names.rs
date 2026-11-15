//! `@@map` / `@map` across a diff (cratestack#1375). Tables and columns are
//! matched by name, so a changed storage name would read as a drop and a
//! create: the table's rows gone, the column's data gone. Refuse it, and name
//! the `@@rename(from = "...")` / `@rename(from = "...")` that says it is a
//! rename.

use super::storage_unrecorded as unrecorded;
use crate::convert::TableProjection;
use crate::error::MigrateError;
use crate::naming::{column_name, table_name};
use crate::projection::Projections;

/// Refuses a diff in which a model's table, or a field's column, is stored
/// under another name and no rename marker says so. A table the previous side
/// has no trace of, and a field it has no column for, are new, not changed.
pub(crate) fn check_storage_names_stable(
    prev: &Projections,
    next: &Projections,
) -> Result<(), MigrateError> {
    for table in next.tables.values() {
        let Some(model) = table.model.as_deref() else {
            continue;
        };
        let Some(was) = previous_table(prev, table, model) else {
            unrecorded::check_new_table(prev, next, table, model)?;
            continue;
        };
        if was.name != table.name && table.rename_from.as_deref() != Some(was.name.as_str()) {
            return Err(MigrateError::TableStorageNameChanged {
                model: model.to_owned(),
                from: was.name.clone(),
                to: table.name.clone(),
            });
        }
        check_columns(was, table, model)?;
        unrecorded::check_columns(was, table, model)?;
    }
    Ok(())
}

/// The previous side's table for `table`'s model: the one that stored the same
/// model, else the one with the same name (the model was renamed, its table
/// not), else, for a snapshot written before `model` was recorded, the one
/// named for the model's derived default.
pub(super) fn previous_table<'a>(
    prev: &'a Projections,
    table: &TableProjection,
    model: &str,
) -> Option<&'a TableProjection> {
    prev.tables
        .values()
        .find(|was| was.model.as_deref() == Some(model))
        .or_else(|| prev.tables.get(&table.name))
        .or_else(|| {
            prev.tables
                .get(&table_name(model))
                .filter(|was| was.model.is_none())
        })
}

fn check_columns(
    was: &TableProjection,
    table: &TableProjection,
    model: &str,
) -> Result<(), MigrateError> {
    for (field, now) in &table.field_columns {
        let Some(before) = previous_column(was, field) else {
            continue;
        };
        let renamed = table
            .column_renames
            .iter()
            .any(|(new, old)| new == now && *old == before);
        if before != *now && !renamed {
            return Err(MigrateError::ColumnStorageNameChanged {
                table: table.name.clone(),
                model: model.to_owned(),
                field: field.clone(),
                from: before,
                to: now.clone(),
            });
        }
    }
    Ok(())
}

/// The column `field` was stored in. A snapshot with no `field_columns` is read
/// as every field under its derived name, when the table has such a column.
fn previous_column(was: &TableProjection, field: &str) -> Option<String> {
    if let Some(column) = was.field_columns.get(field) {
        return Some(column.clone());
    }
    let derived = column_name(field);
    (was.field_columns.is_empty() && was.columns.iter().any(|c| c.name == derived))
        .then_some(derived)
}
