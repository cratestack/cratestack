//! Shared helper for running an INSERT/UPDATE/DELETE/UPSERT RETURNING
//! single-row statement against a caller-supplied connection.

use cratestack_sql::SqlValue;
use rusqlite::params_from_iter;

use crate::{FromRusqliteRow, RusqliteError, SqlValueParam};

pub(super) fn run_insert_returning<M: FromRusqliteRow>(
    conn: &rusqlite::Connection,
    sql: &str,
    binds: &[SqlValue],
) -> Result<M, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let bind_iter = binds.iter().map(SqlValueParam);
    let mut rows = stmt.query(params_from_iter(bind_iter))?;
    let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
    Ok(M::from_rusqlite_row(row)?)
}
