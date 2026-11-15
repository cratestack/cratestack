//! Collisions between final storage names (cratestack#1375).
//!
//! `@@map` and `@map` can make two names equal that the raw-name checks
//! (`super::snake_case_collisions`, `super::route_collisions`) never compare:
//! `@@map("deposits")` beside a model `Deposit`, or `@map("x")` beside a
//! field `x`. Tables are unique across models and views; columns are unique
//! within a model, counting the implicit `deleted_at` of `@@soft_delete`.
//! Only pairs with a mapped member are reported here, so a schema without
//! `@@map` / `@map` keeps the messages it always had. Column collisions
//! between fields are `validate_stored_column_collisions` here, which compares
//! final column names of *stored* fields only; the raw-name guard
//! (`validate_field_column_collisions`, `to_snake_case` of every field, which
//! is what the generated identifiers derive from) is unchanged and runs first.

use std::collections::{BTreeMap, BTreeSet};

use cratestack_core::route_naming::{pluralize, to_snake_case};
use cratestack_core::{Field, Model, Schema, is_computed_field, is_relation_attribute};

use crate::diagnostics::{SchemaError, span_error};
use super::snake_case_collisions::find_collision_by;

fn is_mapped(model: &Model) -> bool {
    model.attributes.iter().any(|a| a.raw.starts_with("@@map"))
}

/// Two models, or a model and a view, stored under one table name.
pub(super) fn validate_table_name_collisions(schema: &Schema) -> Result<(), SchemaError> {
    // (table, declaration, mapped)
    let mut seen: BTreeMap<String, (String, bool)> = BTreeMap::new();
    let entries = schema
        .models
        .iter()
        .map(|m| {
            (
                m.sql_table_name(),
                format!("model `{}`", m.name),
                is_mapped(m),
                m.name_span,
            )
        })
        .chain(schema.views.iter().map(|v| {
            let table = pluralize(&to_snake_case(&v.name));
            (table, format!("view `{}`", v.name), false, v.name_span)
        }));
    for (table, declaration, mapped, span) in entries {
        if let Some((existing, existing_mapped)) = seen.get(&table)
            && (mapped || *existing_mapped)
        {
            return Err(span_error(
                format!(
                    "{declaration} is stored in table `{table}`, which {existing} already uses \
                     (`@@map` names the table; a model or view without one is stored in its \
                     pluralized snake_case name). Give one of them another table name"
                ),
                span,
            ));
        }
        seen.entry(table).or_insert((declaration, mapped));
    }
    Ok(())
}

/// Whether the model carries block attribute `@@<name>`, by its name.
fn has_block_attribute(model: &Model, name: &str) -> bool {
    model.attributes.iter().any(|a| {
        a.raw
            .strip_prefix("@@")
            .and_then(|rest| rest.strip_prefix(name))
            .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
    })
}

/// A field with a column of its own: not a relation or its back reference
/// (a field typed as a model), not `@computed`.
fn is_stored(field: &Field, model_names: &BTreeSet<&str>) -> bool {
    !model_names.contains(field.ty.name.as_str())
        && !is_computed_field(field)
        && !field.attributes.iter().any(is_relation_attribute)
}

/// Two stored fields of one model in the same final column.
pub(super) fn validate_stored_column_collisions(
    model: &Model,
    model_names: &BTreeSet<&str>,
) -> Result<(), SchemaError> {
    let stored: Vec<&Field> = model
        .fields
        .iter()
        .filter(|field| is_stored(field, model_names))
        .collect();
    let columns: BTreeMap<&str, String> = stored
        .iter()
        .map(|field| (field.name.as_str(), field.sql_column_name()))
        .collect();
    let entries = stored.iter().map(|f| (f.name.as_str(), f.span));
    if let Some((existing, colliding, span, column)) =
        find_collision_by(entries, |name| columns[name].clone())
    {
        return Err(span_error(
            format!(
                "field `{colliding}` on model `{}` collides with field `{existing}` — both are \
                 stored in the SQL column `{column}` (`@map` name if any, else the snake_case of \
                 the field name); rename one of them or give it another `@map`",
                model.name
            ),
            span,
        ));
    }
    Ok(())
}

/// `@@soft_delete` stores the deletion time in a column named `deleted_at`.
/// A field mapped *to* it, or the field that derives it (`deletedAt`) mapped
/// away from it, would leave that column to something that is not the model's.
pub(super) fn validate_soft_delete_column(model: &Model) -> Result<(), SchemaError> {
    if !has_block_attribute(model, "soft_delete") {
        return Ok(());
    }
    for field in &model.fields {
        let mapped = field.attributes.iter().any(|a| a.raw.starts_with("@map"));
        if !mapped {
            continue;
        }
        let message = if field.sql_column_name() == "deleted_at" {
            "is mapped to column `deleted_at`, which `@@soft_delete` adds to this model's table. \
             Choose another column name"
        } else if to_snake_case(&field.name) == "deleted_at" {
            "is mapped away from column `deleted_at`, which `@@soft_delete` reads and writes by \
             that name. Remove the `@map`"
        } else {
            continue;
        };
        return Err(span_error(
            format!("field `{}` on model `{}` {message}", field.name, model.name),
            field.span,
        ));
    }
    Ok(())
}
