//! Storage-name markers: a model's `@@map("<table>")` and a stored field's
//! `@map("<column>")` (cratestack#1375).
//!
//! Their one reader is `Model::sql_table_name` / `Field::sql_column_name`
//! (`cratestack_core::schema::storage_names`), which takes the *first* marker
//! whose whole text it can read and falls back to the derived name for
//! anything else. An unread marker would therefore keep the derived name
//! while looking mapped, so every marker is checked with that same reader:
//!
//! - **Form**: `@@map` / `@map`, directly `(`, one double-quoted name, `)`.
//! - **Grammar**: `^[a-z_][a-z0-9_]*$`, at most 63 bytes, not a reserved SQL
//!   word, a table not starting `cratestack_`, `sqlite_` or `pg_`
//!   (`validate_sql_identifier`). Generated SQL does not quote names.
//! - **Placement**: `@map` is read only on a stored column of a model or
//!   mixin. On a relation field, a `@computed` field, a view, `type` or
//!   `auth` field it would rename nothing and is refused (the class of
//!   accepted-but-inert attribute ADR 0019 D5 closes). `@@map` on a view is
//!   refused by the closed `@@` list (`super::block_attributes`).
//! - **At most one** per model or field, the span on the second.
//!
//! Collisions between final names are `super::storage_map_collisions`.

use std::collections::BTreeSet;

use cratestack_core::schema::{
    MAP_ARGUMENT_FORM, StorageNameKind, map_marker_name, validate_sql_identifier,
};
use cratestack_core::{Attribute, Field, Model, is_computed_field, is_relation_attribute};

use crate::diagnostics::{SchemaError, span_error};

/// Whether `raw` is written with the attribute name `map`, whatever follows.
fn is_field_map(raw: &str) -> bool {
    raw.strip_prefix("@map")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

fn is_model_map(raw: &str) -> bool {
    raw.strip_prefix("@@map")
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// Checks one marker's form and name, naming `owner`.
fn check_marker(
    owner: &str,
    marker: &str,
    kind: StorageNameKind,
    attribute: &Attribute,
) -> Result<(), SchemaError> {
    let Some(name) = map_marker_name(&attribute.raw, marker) else {
        return Err(span_error(
            format!(
                "{owner} writes `{}`: `{marker}` takes exactly one argument, \
                 `{marker}({MAP_ARGUMENT_FORM})` — a double-quoted SQL name, for example \
                 `{marker}(\"legacy_name\")`, written directly after `{marker}` and ending the \
                 attribute. Nothing else is read, so any other form would leave the derived \
                 name in use. It is refused",
                attribute.raw
            ),
            attribute.span,
        ));
    };
    validate_sql_identifier(name, kind).map_err(|why| {
        span_error(
            format!("{owner} writes `{}`: {why}", attribute.raw),
            attribute.span,
        )
    })
}

fn refuse_second(owner: &str, marker: &str, first: &Attribute, second: &Attribute) -> SchemaError {
    span_error(
        format!(
            "{owner} declares a second `{marker}`, `{}`, after `{}`: only the first is read, \
             so the second would be silently ignored. Declare one `{marker}`",
            second.raw, first.raw
        ),
        second.span,
    )
}

/// Why a `@map` on `field` of an `owner_kind` block renames nothing, or
/// `None` where it is read.
fn inert_reason(owner_kind: &str, field: &Field) -> Option<&'static str> {
    match owner_kind {
        "model" | "mixin" => {
            if field.attributes.iter().any(is_relation_attribute) {
                Some("a relation field is not a column")
            } else if is_computed_field(field) {
                Some("a `@computed` field is not stored")
            } else {
                None
            }
        }
        "view" => Some("a view's columns come from its SQL"),
        _ => Some("the block is not a table"),
    }
}

/// A field's `@map`: only on a stored column of a model or mixin, in the one
/// form, a valid name, at most once.
pub(super) fn validate_field_map(
    owner_kind: &str,
    owner_name: &str,
    field: &Field,
) -> Result<(), SchemaError> {
    let owner = format!("field `{}` on {owner_kind} `{owner_name}`", field.name);
    let mut first: Option<&Attribute> = None;
    for attribute in field.attributes.iter().filter(|a| is_field_map(&a.raw)) {
        if let Some(why) = inert_reason(owner_kind, field) {
            return Err(span_error(
                format!(
                    "{owner} writes `{}`, which has no effect here: `@map` names the SQL column \
                     of a stored field of a model, and {why}. It is refused; remove it",
                    attribute.raw
                ),
                attribute.span,
            ));
        }
        check_marker(&owner, "@map", StorageNameKind::Column, attribute)?;
        if let Some(first) = first {
            return Err(refuse_second(&owner, "@map", first, attribute));
        }
        first = Some(attribute);
    }
    Ok(())
}

/// `@map` on a field whose type is a model: the back-reference of a relation,
/// which has no column.
pub(super) fn validate_map_not_on_relation(
    owner_name: &str,
    field: &Field,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let Some(attribute) = field.attributes.iter().find(|a| is_field_map(&a.raw)) else {
        return Ok(());
    };
    if !model_names.contains(field.ty.name.as_str()) {
        return Ok(());
    }
    Err(span_error(
        format!(
            "field `{}` on model `{owner_name}` writes `{}`, which has no effect here: `@map` \
             names the SQL column of a stored field, and a relation field is not a column. It \
             is refused; remove it",
            field.name, attribute.raw
        ),
        attribute.span,
    ))
}

/// A model's `@@map`: the one form, a valid table name, at most once. Runs
/// after the closed `@@` list has checked that the attribute has arguments.
pub(super) fn validate_model_map(model: &Model) -> Result<(), SchemaError> {
    let owner = format!("model `{}`", model.name);
    let mut first: Option<&Attribute> = None;
    for attribute in model.attributes.iter().filter(|a| is_model_map(&a.raw)) {
        check_marker(&owner, "@@map", StorageNameKind::Table, attribute)?;
        if let Some(first) = first {
            return Err(refuse_second(&owner, "@@map", first, attribute));
        }
        first = Some(attribute);
    }
    Ok(())
}
