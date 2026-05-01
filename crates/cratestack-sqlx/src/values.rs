use cratestack_core::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Uuid(uuid::Uuid),
    DateTime(chrono::DateTime<chrono::Utc>),
    Json(Value),
    NullBool,
    NullInt,
    NullFloat,
    NullString,
    NullBytes,
    NullUuid,
    NullDateTime,
    NullJson,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FilterValue {
    None,
    Single(SqlValue),
    Many(Vec<SqlValue>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SqlColumnValue {
    pub column: &'static str,
    pub value: SqlValue,
}

pub trait CreateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
}

pub trait UpdateModelInput<M> {
    fn sql_values(&self) -> Vec<SqlColumnValue>;
}

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
