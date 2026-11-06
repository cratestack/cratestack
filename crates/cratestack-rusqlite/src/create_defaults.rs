//! What the embedded runtime does with a model's `@default`s on create.
//!
//! `now()`, `uuid()` and `cuid()` (cratestack#1209) and literal defaults
//! (cratestack#1234) are bound by the runtime, so the value never depends
//! on a `DEFAULT` clause an older table may lack. A `dbgenerated()` or
//! registered-function default is the database's: the create input takes
//! it as an `Option`, and when the input leaves it out the column is left
//! to an app trigger or `DEFAULT`. A required one that nothing filled is
//! refused rather than written as NULL.

use cratestack_sql::{DatabaseDefault, ModelDescriptor, SqlColumnValue};

/// Fill every default the runtime generates or binds itself.
pub(crate) fn fill_create_defaults<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    values: &mut Vec<SqlColumnValue>,
) {
    descriptor.fill_generated_defaults(values);
    descriptor.fill_literal_defaults(values);
}

/// A required column whose database default filled nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnfilledDefault {
    pub column: &'static str,
    pub default: &'static str,
}

impl std::fmt::Display for UnfilledDefault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "column `{}` is required and its `@default({})` is the database's to fill, but the \
             insert left it NULL. On the embedded runtime nothing fills it unless your app adds \
             a trigger or a DEFAULT through `with_connection`; set it in the create input instead",
            self.column, self.default,
        )
    }
}

impl std::error::Error for UnfilledDefault {}

/// The first required database-filled column the row returned by an
/// `INSERT … RETURNING` of the full projection holds as NULL.
pub(crate) fn unfilled_default<M, PK>(
    descriptor: &ModelDescriptor<M, PK>,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Option<UnfilledDefault>> {
    for DatabaseDefault { column, default } in descriptor.database_defaults {
        let Some(index) = descriptor
            .columns
            .iter()
            .position(|c| c.sql_name == *column)
        else {
            continue;
        };
        if row.get_ref(index)? == rusqlite::types::ValueRef::Null {
            return Ok(Some(UnfilledDefault { column, default }));
        }
    }
    Ok(None)
}
