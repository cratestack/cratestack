//! Collisions between final storage names (cratestack#1375).
//!
//! `@@map` and `@map` can make two names equal that the raw-name checks
//! (`super::snake_case_collisions`, `super::route_collisions`) never compare:
//! `@@map("deposits")` beside a model `Deposit`, or `@map("x")` beside a
//! field `x`. Tables are unique across models and views; columns are unique
//! within a model, counting the implicit `deleted_at` of `@@soft_delete`.
//! Only pairs with a mapped member are reported here, so a schema without
//! `@@map` / `@map` keeps the messages it always had. Column collisions
//! between fields are `validate_field_column_collisions`, which compares
//! final column names.

use std::collections::BTreeMap;

use cratestack_core::route_naming::{pluralize, to_snake_case};
use cratestack_core::{Model, Schema};

use crate::diagnostics::{SchemaError, span_error};

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

/// A mapped field stored in the implicit `deleted_at` column of a
/// `@@soft_delete` model.
pub(super) fn validate_soft_delete_column(model: &Model) -> Result<(), SchemaError> {
    if !model.attributes.iter().any(|a| a.raw == "@@soft_delete") {
        return Ok(());
    }
    for field in &model.fields {
        let mapped = field.attributes.iter().any(|a| a.raw.starts_with("@map"));
        if mapped && field.sql_column_name() == "deleted_at" {
            return Err(span_error(
                format!(
                    "field `{}` on model `{}` is mapped to column `deleted_at`, which \
                     `@@soft_delete` adds to this model's table. Choose another column name",
                    field.name, model.name
                ),
                field.span,
            ));
        }
    }
    Ok(())
}
