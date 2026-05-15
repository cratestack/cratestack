//! Partial-row decoder trait — see [`crate::FindMany::select`] for
//! the typed-builder side of the column-projection feature.

use crate::sqlx;

/// Companion to [`sqlx::FromRow`] that decodes a row projected by
/// `.select(...)` — i.e. a row where only the named columns are
/// present in the SQL `SELECT` list. Non-selected fields populate to
/// their type's `Default::default()` value.
///
/// The macro emits this impl for every generated model struct, so the
/// trait is invisible to schema authors at the call site; it shows up
/// as the bound on the typed builder's `T` generic.
pub trait FromPartialPgRow: Sized {
    /// Decode `row` into `Self` using `selected` as the projection
    /// manifest. `selected` carries the SQL column names
    /// (snake_case) the caller asked for; any column not in this
    /// list falls through to `Default::default()`.
    fn decode_partial_pg_row(
        row: &sqlx::postgres::PgRow,
        selected: &[&str],
    ) -> Result<Self, sqlx::Error>;
}
