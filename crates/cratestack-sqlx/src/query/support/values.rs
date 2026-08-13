//! Value-shaped helpers: `SqlValue` → bind-slot push, `auth_field`
//! lookup with type narrowing, slice-of-columns scan, and the two
//! equality checks shared by the create-policy evaluator.

use cratestack_core::{CoolContext, Value};

use crate::{Json, PolicyLiteral, SqlColumnValue, SqlValue, sqlx};

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
        SqlValue::Json(value) => query.push_bind(Json(value.clone())),
        // `cratestack_core::Decimal` is `Copy` under the `decimal-rust-decimal`
        // backend but NOT under `decimal-bigdecimal` (`bigdecimal::BigDecimal`
        // heap-allocates its digit buffer) — `.clone()` is required here to stay
        // backend-agnostic; it degrades to a cheap bitwise copy under the
        // `rust_decimal` backend and a real allocation under `bigdecimal`.
        // `clippy::clone_on_copy` only fires under the `decimal-rust-decimal`
        // build (where `Decimal` happens to be `Copy`) — silenced because the
        // "just dereference it" suggestion doesn't compile at all under
        // `decimal-bigdecimal`, and this call site has to work under both.
        //
        // `cratestack_core::Decimal` only exists when a decimal backend is
        // selected (cratestack#505) — see `cratestack-core/src/decimal.rs`'s
        // module doc.
        #[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
        #[allow(clippy::clone_on_copy)]
        SqlValue::Decimal(value) => query.push_bind(value.clone()),
        SqlValue::NullBool => query.push_bind(Option::<bool>::None),
        SqlValue::NullInt => query.push_bind(Option::<i64>::None),
        SqlValue::NullFloat => query.push_bind(Option::<f64>::None),
        SqlValue::NullString => query.push_bind(Option::<String>::None),
        SqlValue::NullBytes => query.push_bind(Option::<Vec<u8>>::None),
        SqlValue::NullUuid => query.push_bind(Option::<uuid::Uuid>::None),
        SqlValue::NullDateTime => query.push_bind(Option::<chrono::DateTime<chrono::Utc>>::None),
        SqlValue::NullJson => query.push_bind(Option::<Json<Value>>::None),
        #[cfg(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal"))]
        SqlValue::NullDecimal => query.push_bind(Option::<cratestack_core::Decimal>::None),
        // `NullDecimal` itself stays an unconditional `SqlValue` variant
        // (it carries no `Decimal` payload, unlike `Decimal` above), but
        // binding it needs the `Decimal` *type* to name a concrete
        // `Option<T>`. Reaching this arm with no decimal backend selected
        // on this crate means an `SqlValue::NullDecimal` was constructed
        // without going through cratestack-macros' generated code, which
        // can't exist for a schema with a `Decimal` field unless a
        // backend is selected end-to-end (the same invariant the
        // `pgvector`-off arm below already relies on for `Vector`/
        // `NullVector`) — an upstream invariant violation, not a case to
        // handle gracefully.
        #[cfg(not(any(feature = "decimal-rust-decimal", feature = "decimal-bigdecimal")))]
        SqlValue::NullDecimal => unreachable!(
            "SqlValue::NullDecimal requires a decimal backend Cargo feature \
             (decimal-rust-decimal or decimal-bigdecimal) on cratestack-sqlx"
        ),
        #[cfg(feature = "pgvector")]
        SqlValue::Vector(value) => query.push_bind(pgvector::Vector::from(value.clone())),
        #[cfg(feature = "pgvector")]
        SqlValue::NullVector => query.push_bind(Option::<pgvector::Vector>::None),
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
