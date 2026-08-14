use cratestack_core::Value;

use super::sql_value::SqlValue;

pub trait IntoSqlValue {
    fn into_sql_value(self) -> SqlValue;
}

impl IntoSqlValue for bool {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Bool(self)
    }
}

impl IntoSqlValue for i64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Int(self)
    }
}

impl IntoSqlValue for f64 {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Float(self)
    }
}

impl IntoSqlValue for String {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::String(self)
    }
}

impl IntoSqlValue for &str {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::String(self.to_owned())
    }
}

impl IntoSqlValue for uuid::Uuid {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Uuid(self)
    }
}

impl IntoSqlValue for chrono::DateTime<chrono::Utc> {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::DateTime(self)
    }
}

impl IntoSqlValue for Value {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Json(self)
    }
}

// One `impl` per concrete backend (cratestack#505 Direction 2) rather than
// one `impl for cratestack_core::Decimal` — both may be active in the same
// build now (see `cratestack-core/src/decimal.rs`'s module doc), and each
// backend's concrete type boxes into the same `SqlValue::Decimal` variant.
// Not a blanket `impl<D: DecimalValue> IntoSqlValue for D` because that
// would overlap with the concrete impls above (e.g. `i64` already
// satisfies `DecimalValue`'s bounds).
#[cfg(feature = "decimal-rust-decimal")]
impl IntoSqlValue for rust_decimal::Decimal {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Decimal(Box::new(self))
    }
}

#[cfg(feature = "decimal-bigdecimal")]
impl IntoSqlValue for bigdecimal::BigDecimal {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Decimal(Box::new(self))
    }
}

impl IntoSqlValue for Vec<f32> {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Vector(self)
    }
}
