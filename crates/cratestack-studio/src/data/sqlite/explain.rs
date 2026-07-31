//! Plan-only `EXPLAIN QUERY PLAN` for the SQLite source.
//!
//! SQLite spells this differently from Postgres, and the difference
//! matters: bare `EXPLAIN` in SQLite dumps the *bytecode program* —
//! `OpenRead`, `SeekGE`, `IdxGT` — which is a debugging aid for SQLite
//! itself, not an answer to "will this use my index?". `EXPLAIN QUERY
//! PLAN` is the high-level shape (`SCAN`, `SEARCH … USING INDEX …`)
//! that corresponds to what Postgres's `EXPLAIN` shows, so that is what
//! Studio asks for.
//!
//! Neither form executes the statement. SQLite has no `EXPLAIN
//! ANALYZE`, so the "never run the user's mutation" hazard has no
//! spelling here at all; mutations are still refused for the
//! uniformity and defence-in-depth reasons in
//! [`crate::data::EXPLAIN_READ_ONLY_NOTE`].

use rusqlite::Connection;

use crate::data::model_info::ModelSqlInfo;
use crate::data::{
    DEFAULT_PAGE_LIMIT, DataError, EXPLAIN_NEEDS_PK_NOTE, EXPLAIN_READ_ONLY_NOTE, QueryPlan, SqlOp,
};

use super::sql::{build_get_sql, build_list_sql};

/// Build the statement to plan, or the reason we won't. Split out from
/// the connection work so it can be unit-tested without a database.
pub(super) fn plan_request(
    info: &ModelSqlInfo<'_>,
    op: SqlOp,
    pk: Option<&str>,
) -> Result<(String, Option<String>), QueryPlan> {
    match op {
        SqlOp::List => Ok((build_list_sql(info, DEFAULT_PAGE_LIMIT), None)),
        SqlOp::Get => match pk {
            Some(value) => Ok((build_get_sql(info), Some(value.to_owned()))),
            None => Err(QueryPlan::note(EXPLAIN_NEEDS_PK_NOTE)),
        },
        SqlOp::Create | SqlOp::Update | SqlOp::Delete => {
            Err(QueryPlan::note(EXPLAIN_READ_ONLY_NOTE))
        }
    }
}

pub(super) fn explain_blocking(
    conn: &Connection,
    sql: &str,
    bind: Option<String>,
) -> Result<QueryPlan, DataError> {
    let explained = format!("EXPLAIN QUERY PLAN {sql}");
    let mut stmt = conn.prepare(&explained)?;
    // Column 3 (`detail`) is the human-readable line; 0..2 are the
    // node id / parent id / an unused legacy column that together
    // describe the tree we're deliberately flattening.
    let mut iter = stmt.query([bind])?;
    let mut lines = Vec::new();
    while let Some(row) = iter.next()? {
        if let Ok(detail) = row.get::<_, String>(3) {
            lines.push(detail);
        }
    }
    if lines.is_empty() {
        return Ok(QueryPlan::note("SQLite returned an empty query plan."));
    }
    Ok(QueryPlan::text(lines.join("\n")))
}
