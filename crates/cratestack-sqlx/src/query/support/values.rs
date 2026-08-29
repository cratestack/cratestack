//! Value-shaped helpers: `SqlValue` → bind-slot push, `auth_field`
//! lookup with type narrowing, slice-of-columns scan, and the two
//! equality checks shared by the create-policy evaluator.

use cratestack_core::{CratestackContext, Value};

use crate::{Json, PolicyLiteral, SqlColumnValue, SqlValue, sqlx};

use super::decimal_bind::{bind_decimal, bind_null_decimal};

pub(crate) fn push_bind_value(query: &mut sqlx::QueryBuilder<sqlx::Postgres>, value: &SqlValue) {
    // Every arm is a statement (trailing `;`), not a `match`-expression
    // value: `bind_decimal`/`bind_null_decimal` can't return `&mut
    // QueryBuilder` without a lifetime parameter this free function has no
    // clean way to name (two independently-elided input lifetimes, no
    // `&self` to anchor on) — discarding each arm's value sidesteps that
    // entirely, at the cost of losing `push_bind`'s method-chaining value,
    // which nothing here used anyway.
    match value {
        SqlValue::Bool(value) => {
            query.push_bind(*value);
        }
        SqlValue::Int(value) => {
            query.push_bind(*value);
        }
        SqlValue::Float(value) => {
            query.push_bind(*value);
        }
        SqlValue::String(value) => {
            query.push_bind(value.clone());
        }
        SqlValue::Bytes(value) => {
            query.push_bind(value.clone());
        }
        SqlValue::Uuid(value) => {
            query.push_bind(*value);
        }
        SqlValue::DateTime(value) => {
            query.push_bind(*value);
        }
        SqlValue::Json(value) => {
            query.push_bind(Json(value.clone()));
        }
        // `SqlValue::Decimal` holds a `Box<dyn DecimalLike>` (cratestack#505
        // Direction 2), not a fixed concrete type, so this boundary has to
        // downcast to whichever concrete backend(s) this crate's own
        // `decimal-*` features enabled before it can call `push_bind` — sqlx
        // binds a concrete, `Encode`-implementing type, not a trait object.
        // See `bind_decimal` below.
        SqlValue::Decimal(value) => bind_decimal(query, value.as_ref()),
        SqlValue::NullBool => {
            query.push_bind(Option::<bool>::None);
        }
        SqlValue::NullInt => {
            query.push_bind(Option::<i64>::None);
        }
        SqlValue::NullFloat => {
            query.push_bind(Option::<f64>::None);
        }
        SqlValue::NullString => {
            query.push_bind(Option::<String>::None);
        }
        SqlValue::NullBytes => {
            query.push_bind(Option::<Vec<u8>>::None);
        }
        SqlValue::NullUuid => {
            query.push_bind(Option::<uuid::Uuid>::None);
        }
        SqlValue::NullDateTime => {
            query.push_bind(Option::<chrono::DateTime<chrono::Utc>>::None);
        }
        SqlValue::NullJson => {
            query.push_bind(Option::<Json<Value>>::None);
        }
        SqlValue::NullDecimal => bind_null_decimal(query),
        #[cfg(feature = "pgvector")]
        SqlValue::Vector(value) => {
            query.push_bind(pgvector::Vector::from(value.clone()));
        }
        #[cfg(feature = "pgvector")]
        SqlValue::NullVector => {
            query.push_bind(Option::<pgvector::Vector>::None);
        }
        // `Vector(n)`/`pgvector::Vector` requires the `pgvector` Cargo
        // feature on this crate. Reaching here without it means an
        // `SqlValue::Vector`/`NullVector` was constructed without
        // going through cratestack-macros' generated code, which
        // itself can't exist unless the matching feature is enabled
        // end-to-end (#161's compile-time gate) — an upstream
        // invariant violation, not a case to handle gracefully.
        #[cfg(not(feature = "pgvector"))]
        SqlValue::Vector(_) | SqlValue::NullVector => unreachable!(
            "SqlValue::Vector/NullVector requires the `pgvector` Cargo feature on \
             cratestack-sqlx"
        ),
    }
}

pub(crate) fn auth_value_to_sql(ctx: &CratestackContext, auth_field: &str) -> Option<SqlValue> {
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
