//! The `DEFAULT` clause `create_table_sql` writes for a column
//! (cratestack#1234), for rows inserted with raw SQL through
//! `with_connection`. cratestack's own creates bind these values
//! themselves (`crate::create_defaults`), so a table created by an older
//! version without them still gets them.
//!
//! A literal renders from the `SqlValue` the runtime binds for it, through
//! the same binder, so the stored value has the storage class the decoder
//! expects. `now()` and `uuid()` use the expressions migrate's SQLite tree
//! writes. `cuid()`, `dbgenerated()` and registered functions get none.

use cratestack_sql::{ModelDescriptor, SqlValue};
use rusqlite::ToSql;
use rusqlite::types::{ToSqlOutput, ValueRef};

use crate::SqlValueParam;

pub(super) fn column_default<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    column: &str,
) -> Option<String> {
    if let Some(generated) = descriptor
        .generated_defaults
        .iter()
        .find(|default| default.column == column)
    {
        return generated
            .value
            .builtin()
            .sqlite_default()
            .map(str::to_owned);
    }
    let literal = descriptor
        .literal_defaults
        .iter()
        .find(|default| default.column == column)?;
    sqlite_literal(&(literal.value)())
}

/// `value` as an SQLite literal, or `None` for NULL.
fn sqlite_literal(value: &SqlValue) -> Option<String> {
    let param = SqlValueParam(value);
    let bound = param.to_sql().ok()?;
    let value = match &bound {
        ToSqlOutput::Borrowed(value) => *value,
        ToSqlOutput::Owned(value) => ValueRef::from(value),
        _ => return None,
    };
    match value {
        ValueRef::Null => None,
        ValueRef::Integer(value) => Some(value.to_string()),
        ValueRef::Real(value) => Some(format!("{value:?}")),
        ValueRef::Text(text) => Some(format!(
            "'{}'",
            String::from_utf8_lossy(text).replace('\'', "''")
        )),
        ValueRef::Blob(bytes) => {
            let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
            Some(format!("X'{hex}'"))
        }
    }
}
