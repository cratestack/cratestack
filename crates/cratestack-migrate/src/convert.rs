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
mod relations;
mod renames;
mod uniques;

use std::collections::{BTreeMap, HashSet};

use cratestack_core::{Model, Schema, parse_composite_id_attribute};

use crate::ir::{AddCheck, AddForeignKey, AddIndex, CheckKind, Column, ColumnArity, ColumnType};
use crate::naming::{check_name, column_name, index_name_unique, table_name};

use checks::{check_kind_slug, collect_check_kinds, field_has_db_enforce};
use fields::{field_has_unique, field_to_column, is_relation_field};
use relations::relation_foreign_key;
use renames::{field_rename_from, model_rename_from};
use uniques::composite_unique_indexes;

/// IR-side projection of a model: the table plus any indexes implied
/// by field-level attributes.
///
/// This is one half of the [`crate::Projections`] seam (see
/// `crate::projection`): anything that can produce a `TableProjection`
/// for every table it knows about — not just [`project_model`] reading
/// a parsed `.cstack` `Schema` — can be diffed with
/// [`crate::diff_projections`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableProjection {
    pub name: String,
    /// Old SQL table name declared via `@@rename(from = "...")`, if
    /// any. Used by the diff engine to match this projection against
    /// the previous schema's projection of the same logical table.
    pub rename_from: Option<String>,
    pub columns: Vec<Column>,
    /// Map from current SQL column name → previous SQL column name,
    /// for fields that carry `@rename(from = "...")`. Empty when
    /// there are no column renames.
    pub column_renames: Vec<(String, String)>,
    pub indexes: Vec<AddIndex>,
    /// CHECK constraints implied by `@db_enforce` on validator
    /// attributes (`@range`, `@length`, `@iso4217`).
    pub checks: Vec<AddCheck>,
    /// Foreign keys promoted from the owning side of a
    /// `@relation(fields:[...], references:[...])` field. Empty for
    /// the "many" side of a relation, which has no physical column.
    pub foreign_keys: Vec<AddForeignKey>,
}

pub(crate) fn project_model(model: &Model, schema: &Schema) -> TableProjection {
    let known_enums: HashSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();
    let known_types: HashSet<&str> = schema.types.iter().map(|t| t.name.as_str()).collect();
    // Variants in declaration order, so the emitted `IN (...)` list
    // reads the same way the `.cstack` enum does.
    let enum_variants: BTreeMap<&str, Vec<String>> = schema
        .enums
        .iter()
        .map(|decl| {
            (
                decl.name.as_str(),
                decl.variants.iter().map(|v| v.name.clone()).collect(),
            )
        })
        .collect();

    let table = table_name(&model.name);
    // `@@rename(from = "...")` and `@rename(from = "...")` take the
    // SQL identifier the developer is renaming, not the PascalCase
    // model name or camelCase field name. This matches the docs and
    // is the more intuitive form: the rename describes what's in the
    // database, not what's in the .cstack source.
    let rename_from = model_rename_from(model);

    // `@@id([field1, field2, ...])` marks a composite primary key: every
    // listed field's column becomes part of a multi-column `PRIMARY KEY`
    // constraint (see `emit::postgres::tables`/`emit::sqlite::tables`,
    // which already join every `primary_key`-flagged column into one
    // constraint). Mutually exclusive with a field-level `@id`, enforced
    // by `cratestack-parser`.
    let composite_id_fields: HashSet<String> = model
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@@id("))
        .and_then(|attribute| parse_composite_id_attribute(&attribute.raw).ok())
        .map(|fields| fields.into_iter().collect())
        .unwrap_or_default();

    let mut columns = Vec::with_capacity(model.fields.len());
    let mut column_renames = Vec::new();
    let mut indexes = Vec::new();
    let mut checks = Vec::new();
    let mut foreign_keys = Vec::new();

    for field in &model.fields {
        if is_relation_field(field) {
            // Relation virtual fields (`@relation`) don't produce a
            // column themselves; the foreign-key column lives on the
            // owning side as a regular scalar field. The relation
            // itself is promoted to a foreign-key constraint here.
            if let Some(fk) = relation_foreign_key(field, schema, &table) {
                foreign_keys.push(fk);
            }
            continue;
        }

        let mut column = field_to_column(field, &known_enums, &known_types);
        if composite_id_fields.contains(field.name.as_str()) {
            column.primary_key = true;
        }
        if let Some(old_name) = field_rename_from(field) {
            column_renames.push((column.name.clone(), column_name(&old_name)));
        }
        if field_has_unique(field) && !column.primary_key {
            indexes.push(AddIndex {
                name: index_name_unique(&table, &[column.name.as_str()]),
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
        if let Some(kind) = enum_check_kind(&column, &enum_variants) {
            checks.push(AddCheck {
                table: table.clone(),
                column: column.name.clone(),
                name: check_name(&table, &column.name, check_kind_slug(&kind)),
                kind,
            });
        }
        columns.push(column);
    }

    // Model-level `@@unique([...])` composite constraints, projected
    // once the columns they reference are known (issue #262).
    indexes.extend(composite_unique_indexes(model, &table, &columns));

    TableProjection {
        name: table,
        rename_from,
        columns,
        column_renames,
        indexes,
        checks,
        foreign_keys,
    }
}

/// The membership constraint that stands in for a native enum type on
/// an enum-typed column (issue #228). Returns `None` for non-enum
/// columns, and for the degenerate case of an enum declared with no
/// variants — `IN ()` is not valid SQL, and there is nothing useful to
/// constrain.
fn enum_check_kind(
    column: &Column,
    enum_variants: &BTreeMap<&str, Vec<String>>,
) -> Option<CheckKind> {
    let ColumnType::Enum(name) = &column.ty else {
        return None;
    };
    let variants = enum_variants.get(name.as_str())?;
    if variants.is_empty() {
        return None;
    }
    Some(CheckKind::Enum {
        variants: variants.clone(),
        list: matches!(column.arity, ColumnArity::List),
    })
}
