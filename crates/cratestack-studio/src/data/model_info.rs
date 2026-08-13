//! Model resolution + SQL-name helpers shared by every data source.
//!
//! Phase 1a kept this logic inline in `postgres.rs`. Phase 1b lifts it
//! into a sibling module so the new SQLite source can reuse it without
//! pulling Postgres-specific types.

use cratestack_core::{Field, Model, ModelEventKind, Schema, TypeArity, parse_emit_attribute};
use cratestack_migrate::{column_name, table_name};

use super::DataError;

/// Project the columns and primary-key info needed to build a SELECT
/// for one model.
#[derive(Debug, Clone)]
pub(crate) struct ModelSqlInfo<'a> {
    pub table: String,
    pub columns: Vec<ColumnInfo<'a>>,
    pub pk_column: String,
    /// The PK field's `.cstack` name. Threaded through for future
    /// callers that need it; not actively read yet — the FollowResult
    /// extraction in `relations::extract_filter_value` uses the
    /// relation's resolved column name directly.
    #[allow(dead_code)]
    pub pk_field_name: &'a str,
    pub pk_cast: PkCast,
}

#[derive(Debug, Clone)]
pub(crate) struct ColumnInfo<'a> {
    /// The field's `.cstack` name (camelCase). Both dialects project
    /// rows keyed by this rather than by `column_name`, which is what
    /// makes [`crate::data::Row`]'s "keys are field names" contract
    /// true: SQLite labels its `json_object(...)` pairs with it, and
    /// Postgres aliases each selected column to it.
    pub field_name: &'a str,
    pub column_name: String,
}

/// How a primary-key value should be interpreted at the SQL layer.
/// Text-shaped PKs (`String`, `Cuid`, `Uuid`, `Decimal`) get bound
/// directly; `Int` PKs get cast on the bound parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkCast {
    Text,
    BigInt,
}

pub(crate) fn resolve_model<'a>(
    schema: &'a Schema,
    model_name: &str,
) -> Result<(&'a Model, ModelSqlInfo<'a>), DataError> {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == model_name)
        .ok_or_else(|| DataError::UnknownModel {
            model: model_name.to_owned(),
        })?;

    let pk_field = find_pk_field(model).ok_or_else(|| DataError::NoPrimaryKey {
        model: model_name.to_owned(),
    })?;

    let columns = model
        .fields
        .iter()
        .filter(|f| is_scalar_field(schema, f))
        .map(|f| ColumnInfo {
            field_name: f.name.as_str(),
            column_name: column_name(&f.name),
        })
        .collect();

    let pk_cast = pk_cast_for(&pk_field.ty.name).ok_or_else(|| DataError::Unsupported {
        what: "primary key of this type (Phase 1 supports String, Cuid, Uuid, Decimal, Int)",
    })?;

    Ok((
        model,
        ModelSqlInfo {
            table: table_name(&model.name),
            columns,
            pk_column: column_name(&pk_field.name),
            pk_field_name: pk_field.name.as_str(),
            pk_cast,
        },
    ))
}

pub(crate) fn find_pk_field(model: &Model) -> Option<&Field> {
    model
        .fields
        .iter()
        .find(|f| f.attributes.iter().any(|a| a.raw.starts_with("@id")))
}

/// The SQL column name of `model`'s `@version` field, if it declares
/// one. The parser guarantees at most one `@version` field per model,
/// that it's a required `Int`, and that it isn't also `@id`
/// (`cratestack-parser/src/validate/model_attributes.rs`), so a column
/// found here is always safe to increment arithmetically.
pub(crate) fn version_column(model: &Model) -> Option<String> {
    model
        .fields
        .iter()
        .find(|f| f.attributes.iter().any(|a| a.raw == "@version"))
        .map(|f| column_name(&f.name))
}

/// The set of operations `model`'s `@@emit(...)` attribute(s) declare,
/// deduplicated. Empty when the model has no `@@emit` at all. Mirrors
/// `cratestack-macros::event::model_emitted_events` (the codegen path's
/// equivalent), reusing the same `cratestack_core::parse_emit_attribute`
/// parser so the two never disagree on what a given `@@emit(...)` string
/// means. Malformed `@@emit(...)` text is treated as declaring nothing —
/// the parser's own semantic checker is what rejects it at schema-load
/// time; this is a runtime read of an already-validated schema.
pub(crate) fn emitted_events(model: &Model) -> Vec<ModelEventKind> {
    let mut emitted = Vec::new();
    for attribute in &model.attributes {
        if attribute.raw.starts_with("@@emit(") {
            emitted.extend(parse_emit_attribute(&attribute.raw).unwrap_or_default());
        }
    }
    emitted.dedup();
    emitted
}

/// A field counts as scalar if its arity isn't `List` and its declared
/// type doesn't name a model in the same schema. Enum-typed fields stay
/// scalar (they're stored as text columns).
pub(crate) fn is_scalar_field(schema: &Schema, field: &Field) -> bool {
    if matches!(field.ty.arity, TypeArity::List) {
        return false;
    }
    !schema.models.iter().any(|m| m.name == field.ty.name)
}

fn pk_cast_for(scalar: &str) -> Option<PkCast> {
    match scalar {
        "String" | "Uuid" | "Cuid" | "Decimal" => Some(PkCast::Text),
        "Int" => Some(PkCast::BigInt),
        _ => None,
    }
}

/// JSON values come back from the row-to-json layer as either strings
/// or numbers depending on column type; both serialize losslessly as a
/// `String` cursor that we later re-bind as text and cast in SQL.
pub(crate) fn json_value_to_cursor(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
