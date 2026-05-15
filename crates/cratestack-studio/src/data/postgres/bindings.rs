//! Typed-value bindings for the Postgres source.
//!
//! Each scalar Postgres column type maps to one [`TypedValue`] variant;
//! the variant is selected from the schema's declared field type
//! rather than the JSON value's runtime shape, so a Postgres `jsonb`
//! column always binds through `sqlx::types::Json` even when the
//! payload happens to be a plain string.

use crate::data::Row;
use crate::data::model_info::ModelSqlInfo;

/// One typed value ready for a Postgres bind. JSON / array / object
/// payloads bind through `sqlx::types::Json` so non-jsonb columns get
/// a hard error at type-check time rather than a silent stringification.
#[derive(Debug, Clone)]
pub(crate) enum TypedValue {
    Text(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    Json(serde_json::Value),
    Null,
}

/// Walk `payload` in column order, looking up each field's scalar
/// type on the source model and producing a [`TypedValue`] for each
/// present key.
pub(super) fn collect_payload(
    schema: &cratestack_core::Schema,
    model_name: &str,
    info: &ModelSqlInfo<'_>,
    payload: &Row,
) -> (Vec<String>, Vec<TypedValue>) {
    let model = schema
        .models
        .iter()
        .find(|m| m.name == model_name)
        .expect("resolve_model already checked the model exists");
    let mut cols = Vec::new();
    let mut binds = Vec::new();
    for col in &info.columns {
        let Some(value) = payload.get(col.field_name) else {
            continue;
        };
        let field = model
            .fields
            .iter()
            .find(|f| f.name == col.field_name)
            .expect("column info was derived from the same field list");
        cols.push(col.column_name.clone());
        binds.push(json_to_typed(&field.ty.name, value));
    }
    (cols, binds)
}

fn json_to_typed(scalar: &str, value: &serde_json::Value) -> TypedValue {
    if value.is_null() {
        return TypedValue::Null;
    }
    match scalar {
        "Int" => TypedValue::Int(value.as_i64().unwrap_or_else(|| {
            value.as_f64().map(|f| f as i64).unwrap_or(0)
        })),
        "Float" => TypedValue::Float(value.as_f64().unwrap_or(0.0)),
        "Boolean" => TypedValue::Bool(value.as_bool().unwrap_or(false)),
        "Json" => TypedValue::Json(value.clone()),
        // String, Cuid, Uuid, Decimal, DateTime, Bytes, enums.
        _ => TypedValue::Text(match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        }),
    }
}

/// Bind one typed value onto a sqlx Query. The `match` keeps the
/// per-variant Encode chosen at compile time even though the caller
/// sees a single function.
pub(super) fn bind_typed<'q>(
    q: sqlx_core::query::Query<
        'q,
        sqlx_postgres::Postgres,
        sqlx_postgres::PgArguments,
    >,
    value: &TypedValue,
) -> sqlx_core::query::Query<
    'q,
    sqlx_postgres::Postgres,
    sqlx_postgres::PgArguments,
> {
    match value {
        TypedValue::Text(s) => q.bind(s.clone()),
        TypedValue::Int(i) => q.bind(*i),
        TypedValue::Float(f) => q.bind(*f),
        TypedValue::Bool(b) => q.bind(*b),
        TypedValue::Json(j) => q.bind(sqlx_core::types::Json(j.clone())),
        TypedValue::Null => q.bind(Option::<String>::None),
    }
}

pub(super) fn typed_kind(value: &TypedValue) -> &'static str {
    match value {
        TypedValue::Text(_) => "text",
        TypedValue::Int(_) => "bigint",
        TypedValue::Float(_) => "double",
        TypedValue::Bool(_) => "boolean",
        TypedValue::Json(_) => "jsonb",
        TypedValue::Null => "null",
    }
}
