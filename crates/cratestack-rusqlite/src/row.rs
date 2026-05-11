//! Row decoding trait for SQLite.
//!
//! Mirrors `sqlx::FromRow<PgRow>` for the rusqlite side. The model macro
//! emits an impl of this trait when targeting the SQLite backend, so user
//! code never sees this directly — it just calls `find_many().run()` and
//! receives `Vec<UserModel>`.

use rusqlite::Row;

/// Decode a model from a rusqlite row.
///
/// Implementations are free to use either positional (`row.get(0)`) or named
/// (`row.get("col")`) lookups. The codegen uses named lookups against the
/// model's `rust_name` aliases, matching the projection produced by
/// [`cratestack_sql::ModelDescriptor::select_projection`].
pub trait FromRusqliteRow: Sized {
    fn from_rusqlite_row(row: &Row<'_>) -> rusqlite::Result<Self>;
}
