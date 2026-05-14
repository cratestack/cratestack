//! Lower a `cratestack_core::Model` into the IR types the diff engine
//! and emitters consume.
//!
//! The conversion is mechanical: model name → table name, field name →
//! column name, field attributes inspected as raw strings to extract
//! `@id`, `@unique`, `@default(…)`. User-defined types and enums are
//! recognised via the schema's `types` and `enums` lists so the emitter
//! can route them to dialect-specific handling.

use std::collections::HashSet;

use cratestack_core::{Field, Model, Schema, TypeArity};

use crate::ir::{AddIndex, Column, ColumnArity, ColumnDefault, ColumnType};
use crate::naming::{column_name, index_name_unique, table_name};

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
        columns.push(column);
    }

    TableProjection {
        name: table,
        rename_from,
        columns,
        column_renames,
        indexes,
    }
}

fn model_rename_from(model: &Model) -> Option<String> {
    let raw = model
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@@rename("))?
        .raw
        .as_str();
    parse_rename_from(raw, "@@rename(")
}

fn field_rename_from(field: &Field) -> Option<String> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@rename("))?
        .raw
        .as_str();
    parse_rename_from(raw, "@rename(")
}

/// Extract the `<old>` value from `@rename(from = "<old>")` or
/// `@@rename(from = "<old>")`. Returns `None` for malformed input —
/// the diff engine treats malformed renames as if the attribute were
/// absent, falling back to drop+add. A future slice can promote this
/// to a parse-time validation error.
fn parse_rename_from(raw: &str, prefix: &str) -> Option<String> {
    let inner = raw.strip_prefix(prefix)?.strip_suffix(')')?.trim();
    let rest = inner.strip_prefix("from")?.trim_start();
    let value_part = rest.strip_prefix('=')?.trim_start();
    let unquoted = value_part
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))?;
    Some(unquoted.to_owned())
}

fn field_to_column(
    field: &Field,
    known_enums: &HashSet<&str>,
    known_types: &HashSet<&str>,
) -> Column {
    let primary_key = field_has_id(field);
    let arity = match field.ty.arity {
        TypeArity::Required => ColumnArity::Required,
        TypeArity::Optional => ColumnArity::Optional,
        TypeArity::List => ColumnArity::List,
    };

    let ty_name = field.ty.name.as_str();
    let ty = if known_enums.contains(ty_name) {
        ColumnType::Enum(ty_name.to_owned())
    } else if known_types.contains(ty_name) {
        ColumnType::UserDefined(ty_name.to_owned())
    } else {
        ColumnType::Scalar(ty_name.to_owned())
    };

    Column {
        name: column_name(&field.name),
        ty,
        arity,
        default: field_default(field),
        primary_key,
    }
}

fn field_has_id(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@id" || attribute.raw.starts_with("@id("))
}

fn field_has_unique(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@unique" || attribute.raw.starts_with("@unique("))
}

fn is_relation_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@relation("))
}

fn field_default(field: &Field) -> Option<ColumnDefault> {
    let raw = field
        .attributes
        .iter()
        .find(|attribute| attribute.raw.starts_with("@default("))?
        .raw
        .as_str();
    // `@default(<inner>)` — strip prefix/suffix, trim. We classify
    // function calls (suffix `()`) vs literals; everything else is
    // passed to the emitter as a literal and quoted per dialect.
    let inner = raw
        .strip_prefix("@default(")?
        .strip_suffix(')')?
        .trim()
        .to_owned();
    if inner.is_empty() {
        return None;
    }
    if inner.ends_with(')') && !inner.starts_with('\'') {
        Some(ColumnDefault::Function(inner))
    } else {
        Some(ColumnDefault::Literal(inner))
    }
}
