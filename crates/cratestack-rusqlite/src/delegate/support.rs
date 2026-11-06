//! Shared helper for running an INSERT/UPDATE/DELETE/UPSERT RETURNING
//! single-row statement against a caller-supplied connection.

use cratestack_sql::{ModelDescriptor, SqlValue};
use rusqlite::params_from_iter;

use crate::create_defaults::unfilled_default;
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

/// [`run_insert_returning`] for a create or an upsert: a row that leaves a
/// required database-filled column NULL is refused, and the insert rolled
/// back (cratestack#1234). Models with no such column skip the savepoint.
pub(super) fn run_create_returning<M: FromRusqliteRow, PK>(
    conn: &rusqlite::Connection,
    descriptor: &ModelDescriptor<M, PK>,
    sql: &str,
    binds: &[SqlValue],
) -> Result<M, RusqliteError> {
    if descriptor.database_defaults.is_empty() {
        return run_insert_returning(conn, sql, binds);
    }
    conn.execute_batch("SAVEPOINT cratestack_create")?;
    let result = insert_checked(conn, descriptor, sql, binds);
    conn.execute_batch(match result {
        Ok(_) => "RELEASE cratestack_create",
        Err(_) => "ROLLBACK TO cratestack_create; RELEASE cratestack_create",
    })?;
    result
}

fn insert_checked<M: FromRusqliteRow, PK>(
    conn: &rusqlite::Connection,
    descriptor: &ModelDescriptor<M, PK>,
    sql: &str,
    binds: &[SqlValue],
) -> Result<M, RusqliteError> {
    let mut stmt = conn.prepare(sql)?;
    let mut rows = stmt.query(params_from_iter(binds.iter().map(SqlValueParam)))?;
    let row = rows.next()?.ok_or(RusqliteError::NotFound)?;
    if let Some(unfilled) = unfilled_default(descriptor, row)? {
        return Err(RusqliteError::UnfilledDefault(unfilled));
    }
    Ok(M::from_rusqlite_row(row)?)
}
