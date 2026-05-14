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

use crate::ir::{AddCheck, AddIndex, CheckKind, Column, ColumnArity, ColumnDefault, ColumnType};
use crate::naming::{check_name, column_name, index_name_unique, table_name};

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

fn field_has_db_enforce(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@db_enforce")
}

/// Collect every eligible validator attribute on `field` as a
/// [`CheckKind`]. Eligibility matches the ADR 0004 list: `@range`,
/// `@length`, `@iso4217`. Validators that don't translate cleanly to
/// SQL (`@email`, `@uri`, `@regex`) are skipped silently here — a
/// future parser-level validation slice can promote `@db_enforce`
/// on an ineligible validator to a parse-time error.
fn collect_check_kinds(field: &Field) -> Vec<CheckKind> {
    let mut out = Vec::new();
    for attribute in &field.attributes {
        let raw = attribute.raw.as_str();
        if let Some(args) = strip_call(raw, "@range") {
            let (min, max) = parse_int_min_max(args);
            out.push(CheckKind::Range { min, max });
        } else if let Some(args) = strip_call(raw, "@length") {
            let (min, max) = parse_int_min_max(args);
            out.push(CheckKind::Length { min, max });
        } else if raw == "@iso4217" {
            out.push(CheckKind::Iso4217);
        }
    }
    out
}

fn check_kind_slug(kind: &CheckKind) -> &'static str {
    match kind {
        CheckKind::Range { .. } => "range",
        CheckKind::Length { .. } => "length",
        CheckKind::Iso4217 => "iso4217",
    }
}

/// `@validator(...)` → `Some("...")`. Returns `None` when `raw` is
/// not a call to `validator`.
fn strip_call<'a>(raw: &'a str, validator: &str) -> Option<&'a str> {
    let after_name = raw.strip_prefix(validator)?;
    let inner = after_name.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner)
}

/// Parse `min: 0, max: 100` / `min: 0` / `max: 100` into `(min, max)`.
/// Tolerates whitespace and missing fields. Returns `(None, None)` on
/// any malformed input — the validator-level parser in
/// `cratestack-parser` has already rejected garbage by this point.
fn parse_int_min_max(args: &str) -> (Option<i64>, Option<i64>) {
    let mut min = None;
    let mut max = None;
    for part in args.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("min") {
            let value = rest.trim_start().strip_prefix(':').map(str::trim);
            if let Some(value) = value.and_then(|v| v.parse::<i64>().ok()) {
                min = Some(value);
            }
        } else if let Some(rest) = part.strip_prefix("max") {
            let value = rest.trim_start().strip_prefix(':').map(str::trim);
            if let Some(value) = value.and_then(|v| v.parse::<i64>().ok()) {
                max = Some(value);
            }
        }
    }
    (min, max)
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
