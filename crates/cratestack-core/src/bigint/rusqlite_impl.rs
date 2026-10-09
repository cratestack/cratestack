//! SQLite driver impls for [`BigInt`], behind the `rusqlite` feature.
//!
//! Both delegate to `i64`: a SQLite integer in, a SQLite integer out. A stored
//! REAL, TEXT or BLOB is refused by `i64`'s `FromSql` rather than coerced, so a
//! column that was written by something other than this framework fails
//! closed.

use rusqlite::types::{FromSql, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use super::BigInt;

impl ToSql for BigInt {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        self.0.to_sql()
    }
}

impl FromSql for BigInt {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        i64::column_result(value).map(Self)
    }
}
