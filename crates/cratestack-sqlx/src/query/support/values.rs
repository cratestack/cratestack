//! Value-shaped helpers: `SqlValue` → bind-slot push, `auth_field`
//! lookup with type narrowing, slice-of-columns scan, and the two
//! equality checks shared by the create-policy evaluator.

use cratestack_core::{CoolContext, Value};

use crate::{PolicyLiteral, SqlColumnValue, SqlValue, sqlx};

pub(crate) fn push_bind_value(
    query: &mut sqlx::QueryBuilder<'_, sqlx::Postgres>,
    value: &SqlValue,
) {
    match value {
        SqlValue::Bool(value) => query.push_bind(*value),
        SqlValue::Int(value) => query.push_bind(*value),
        SqlValue::Float(value) => query.push_bind(*value),
        SqlValue::String(value) => query.push_bind(value.clone()),
        SqlValue::Bytes(value) => query.push_bind(value.clone()),
        SqlValue::Uuid(value) => query.push_bind(*value),
        SqlValue::DateTime(value) => query.push_bind(*value),
        SqlValue::Json(value) => query.push_bind(sqlx::types::Json(value.clone())),
        SqlValue::Decimal(value) => query.push_bind(*value),
        SqlValue::NullBool => query.push_bind(Option::<bool>::None),
        SqlValue::NullInt => query.push_bind(Option::<i64>::None),
        SqlValue::NullFloat => query.push_bind(Option::<f64>::None),
        SqlValue::NullString => query.push_bind(Option::<String>::None),
        SqlValue::NullBytes => query.push_bind(Option::<Vec<u8>>::None),
        SqlValue::NullUuid => query.push_bind(Option::<uuid::Uuid>::None),
        SqlValue::NullDateTime => query.push_bind(Option::<chrono::DateTime<chrono::Utc>>::None),
        SqlValue::NullJson => query.push_bind(Option::<sqlx::types::Json<Value>>::None),
        SqlValue::NullDecimal => query.push_bind(Option::<cratestack_core::Decimal>::None),
    };
}

pub(crate) fn auth_value_to_sql(ctx: &CoolContext, auth_field: &str) -> Option<SqlValue> {
    match ctx.auth_field(auth_field)? {
        Value::Bool(value) => Some(SqlValue::Bool(*value)),
        Value::Int(value) => Some(SqlValue::Int(*value)),
        Value::String(value) => Some(SqlValue::String(value.clone())),
        _ => None,
    }
}

pub(crate) fn find_column_value<'a>(
    values: &'a [SqlColumnValue],
    column: &str,
) -> Option<&'a SqlValue> {
    values
        .iter()
        .find(|value| value.column == column)
        .map(|value| &value.value)
}

pub(crate) fn sql_value_matches_literal(value: &SqlValue, literal: PolicyLiteral) -> bool {
    match (value, literal) {
        (SqlValue::Bool(left), PolicyLiteral::Bool(right)) => *left == right,
        (SqlValue::Int(left), PolicyLiteral::Int(right)) => *left == right,
        (SqlValue::String(left), PolicyLiteral::String(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn value_matches_auth_literal(value: &Value, literal: PolicyLiteral) -> bool {
    match (value, literal) {
        (Value::Bool(left), PolicyLiteral::Bool(right)) => *left == right,
        (Value::Int(left), PolicyLiteral::Int(right)) => *left == right,
        (Value::String(left), PolicyLiteral::String(right)) => left == right,
        _ => false,
    }
}
