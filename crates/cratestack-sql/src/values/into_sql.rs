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

impl IntoSqlValue for cratestack_core::Decimal {
    fn into_sql_value(self) -> SqlValue {
        SqlValue::Decimal(self)
    }
}
