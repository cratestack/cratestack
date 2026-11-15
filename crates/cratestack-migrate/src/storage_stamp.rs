//! Records which model and field an introspected table and column store
//! (cratestack#1375).
//!
//! `migrate baseline` snapshots the live database, and an introspected table
//! has no `model` and no `field_columns`, so the next `migrate diff` could not
//! tell a changed `@@map` / `@map` from an unrelated drop and create. Where the
//! authored schema stores a model in a table that exists, that is the model it
//! is, and the snapshot says so.

use crate::projection::Projections;

/// Stamps `model` and `field_columns` from `authored` onto every table of
/// `introspected` that has the same name and none recorded. A field is stamped
/// only when the introspected table has its column, so the snapshot never
/// claims a column the database lacks. Nothing else changes, so a checksum of
/// the introspected shape taken before is the one taken after for the tables
/// and columns themselves.
pub fn stamp_storage_names(introspected: &mut Projections, authored: &Projections) {
    for (name, table) in &mut introspected.tables {
        let Some(authored_table) = authored.tables.get(name) else {
            continue;
        };
        if table.model.is_some() {
            continue;
        }
        table.model.clone_from(&authored_table.model);
        table.field_columns = authored_table
            .field_columns
            .iter()
            .filter(|(_, column)| table.columns.iter().any(|c| &c.name == *column))
            .map(|(field, column)| (field.clone(), column.clone()))
            .collect();
    }
}

impl crate::TableProjection {
    /// This table without its identity metadata (`model`, `field_columns`).
    /// Those say which model and fields a table stores, not what DDL it has,
    /// so "does the database match the schema" compares without them: an
    /// introspected table never has them.
    pub fn without_storage_identity(&self) -> Self {
        Self {
            model: None,
            field_columns: Default::default(),
            ..self.clone()
        }
    }
}
