//! Per-field projection: scalar/enum/user-defined type detection,
//! arity mapping, default-value parsing.

use std::collections::HashSet;

use cratestack_core::{Field, TypeArity};

use crate::ir::{Column, ColumnArity, ColumnDefault, ColumnType};
use crate::naming::column_name;

pub(super) fn field_to_column(
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
    let ty = if let Some(dimension) = field.ty.vector_dim() {
        ColumnType::Vector(dimension)
    } else if known_enums.contains(ty_name) {
        ColumnType::Enum(ty_name.to_owned())
    } else if known_types.contains(ty_name) {
        ColumnType::UserDefined(ty_name.to_owned())
    } else {
        ColumnType::Scalar(ty_name.to_owned())
    };

    let mut default = field_default(field);
    if matches!(ty, ColumnType::Enum(_)) {
        default = default.map(quote_bare_literal);
    }

    Column {
        name: column_name(&field.name),
        ty,
        arity,
        default,
        primary_key,
    }
}

/// Normalise an enum column's literal default into a quoted SQL string
/// literal.
///
/// Enum variants are written bare in `.cstack` (`@default(pending)`),
/// but a bareword in a `DEFAULT` clause parses as a *column reference*,
/// not a value — Postgres rejects it outright with "cannot use column
/// reference in DEFAULT expression", and SQLite rejects it too. See
/// issue #227.
///
/// Normalising here rather than in each emitter keeps the quoting in
/// one place and keeps both sides of a `prev`/`next` default
/// comparison in the same form, so no spurious `AlterColumnDefault`
/// is produced. Only enum columns are touched: a bare literal on a
/// scalar column may legitimately be an unquoted SQL keyword such as
/// `CURRENT_TIMESTAMP`, which must not be quoted.
fn quote_bare_literal(default: ColumnDefault) -> ColumnDefault {
    match default {
        ColumnDefault::Literal(value) if !is_quoted(&value) => {
            ColumnDefault::Literal(format!("'{}'", value.replace('\'', "''")))
        }
        other => other,
    }
}

fn is_quoted(value: &str) -> bool {
    value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'')
}

/// True for any `@id`-tagged field, with no cap on how many fields on
/// the model this returns true for — `cratestack-parser`'s
/// `validate_models` is what enforces "at most one field-level `@id`"
/// (issue #536), before a schema with more than one ever reaches this
/// function. Two `@id`-tagged columns reaching `project_model` would
/// otherwise silently produce a multi-column `PRIMARY KEY`, bypassing
/// the same #136 restriction `@@id([...])` is rejected for at macro
/// expansion.
fn field_has_id(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@id" || attribute.raw.starts_with("@id("))
}

pub(super) fn field_has_unique(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@unique" || attribute.raw.starts_with("@unique("))
}

pub(super) fn is_relation_field(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@relation("))
}

/// `@computed` / `@computed(params: <Type>?)` fields are resolved at
/// response time and never stored — see `docs/design/computed-fields.md`.
/// They must never produce a column or participate in DDL diffing, the
/// same way relation virtual fields don't (mirrors [`is_relation_field`]).
pub(super) fn is_computed_field(field: &Field) -> bool {
    cratestack_core::is_computed_field(field)
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
    // `dbgenerated()` is a marker, not a real function call — see
    // `ColumnDefault::DbGenerated`. The parser rejects any argument
    // (`validate_default_dbgenerated_no_args`), so the only form that
    // reaches here is the bare call.
    if inner == "dbgenerated()" {
        return Some(ColumnDefault::DbGenerated);
    }
    if inner.ends_with(')') && !inner.starts_with('\'') {
        Some(ColumnDefault::Function(inner))
    } else {
        Some(ColumnDefault::Literal(inner))
    }
}
