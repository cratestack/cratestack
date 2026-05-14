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
    pub(crate) columns: Vec<Column>,
    pub(crate) indexes: Vec<AddIndex>,
}

pub(crate) fn project_model(model: &Model, schema: &Schema) -> TableProjection {
    let known_enums: HashSet<&str> = schema.enums.iter().map(|e| e.name.as_str()).collect();
    let known_types: HashSet<&str> = schema.types.iter().map(|t| t.name.as_str()).collect();

    let table = table_name(&model.name);
    let mut columns = Vec::with_capacity(model.fields.len());
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
        columns,
        indexes,
    }
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
