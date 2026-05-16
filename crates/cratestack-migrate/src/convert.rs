//! Lower a `cratestack_core::Model` into the IR types the diff engine
//! and emitters consume.
//!
//! The conversion is mechanical: model name → table name, field name
//! → column name, field attributes inspected as raw strings to extract
//! `@id`, `@unique`, `@default(…)`. User-defined types and enums are
//! recognised via the schema's `types` and `enums` lists so the
//! emitter can route them to dialect-specific handling.

mod checks;
mod fields;
mod renames;

use std::collections::HashSet;

use cratestack_core::{Model, Schema};

use crate::ir::{AddCheck, AddIndex, Column};
use crate::naming::{check_name, column_name, index_name_unique, table_name};

use checks::{check_kind_slug, collect_check_kinds, field_has_db_enforce};
use fields::{field_has_unique, field_to_column, is_relation_field};
use renames::{field_rename_from, model_rename_from};

/// IR-side projection of a model: the table plus any indexes implied
/// by field-level attributes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableProjection {
    pub(crate) name: String,
    /// Old SQL table name declared via `@@rename(from = "...")`, if
    /// any. Used by the diff engine to match this projection against
    /// the previous schema's projection of the same logical table.
    pub(crate) rename_from: Option<String>,
    pub(crate) columns: Vec<Column>,
    /// Map from current SQL column name → previous SQL column name,
    /// for fields that carry `@rename(from = "...")`. Empty when
    /// there are no column renames.
    pub(crate) column_renames: Vec<(String, String)>,
    pub(crate) indexes: Vec<AddIndex>,
    /// CHECK constraints implied by `@db_enforce` on validator
    /// attributes (`@range`, `@length`, `@iso4217`).
    pub(crate) checks: Vec<AddCheck>,
}

pub(crate) fn project_model(model: &Model, schema: &Schema) -> TableProjection {
    let known_enums: HashSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();
    let known_types: HashSet<&str> = schema.types.iter().map(|t| t.name.as_str()).collect();

    let table = table_name(&model.name);
    // `@@rename(from = "...")` and `@rename(from = "...")` take the
    // SQL identifier the developer is renaming, not the PascalCase
    // model name or camelCase field name. This matches the docs and
    // is the more intuitive form: the rename describes what's in the
    // database, not what's in the .cstack source.
    let rename_from = model_rename_from(model);

    let mut columns = Vec::with_capacity(model.fields.len());
    let mut column_renames = Vec::new();
    let mut indexes = Vec::new();
    let mut checks = Vec::new();

    for field in &model.fields {
        if is_relation_field(field) {
            // Relation virtual fields (`@relation`) don't produce a
            // column themselves; the foreign-key column lives on the
            // owning side as a regular scalar field. Slice 8+ will
            // promote relations to foreign-key IR ops.
            continue;
        }

        let column = field_to_column(field, &known_enums, &known_types);
        if let Some(old_name) = field_rename_from(field) {
            column_renames.push((column.name.clone(), column_name(&old_name)));
        }
        if field_has_unique(field) && !column.primary_key {
            indexes.push(AddIndex {
                name: index_name_unique(&table, &column.name),
                table: table.clone(),
                columns: vec![column.name.clone()],
                unique: true,
            });
        }
        if field_has_db_enforce(field) {
            for kind in collect_check_kinds(field) {
                let validator = check_kind_slug(&kind);
                checks.push(AddCheck {
                    table: table.clone(),
                    column: column.name.clone(),
                    name: check_name(&table, &column.name, validator),
                    kind,
                });
            }
        }
        columns.push(column);
    }

    TableProjection {
        name: table,
        rename_from,
        columns,
        column_renames,
        indexes,
        checks,
    }
}
