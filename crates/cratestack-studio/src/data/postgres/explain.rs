//! Plan-only `EXPLAIN` for the Postgres source.
//!
//! Postgres plans a parameterised statement happily over the extended
//! query protocol, which is what sqlx speaks — so we can hand it the
//! *same* SQL string [`super::sql`] builds for the real query, bind the
//! same parameters, and get back the plan the real query would use.
//! Re-rendering a simplified statement just for EXPLAIN would defeat
//! the point: the plan would be for a query Studio never runs.
//!
//! The options are pinned deliberately. `COSTS true` keeps the row and
//! cost estimates that make a plan worth reading; `FORMAT TEXT` matches
//! what `psql` shows, so the output is familiar and diffable. `ANALYZE`
//! is absent and cannot be switched on — see
//! [`crate::data::EXPLAIN_READ_ONLY_NOTE`].

use sqlx_core::row::Row as _;
use sqlx_postgres::{PgPool, PgRow};

use crate::data::model_info::ModelSqlInfo;
use crate::data::{
    DEFAULT_PAGE_LIMIT, DataError, EXPLAIN_NEEDS_PK_NOTE, EXPLAIN_READ_ONLY_NOTE, QueryPlan, SqlOp,
};

use super::sql::{build_get_sql, build_list_sql};

pub(super) async fn explain(
    pool: &PgPool,
    info: &ModelSqlInfo<'_>,
    op: SqlOp,
    pk: Option<&str>,
) -> Result<QueryPlan, DataError> {
    // Mutations never reach the driver at all.
    let (sql, bind) = match op {
        SqlOp::List => (build_list_sql(info, DEFAULT_PAGE_LIMIT), None),
        SqlOp::Get => match pk {
            Some(value) => (build_get_sql(info), Some(value.to_owned())),
            None => return Ok(QueryPlan::note(EXPLAIN_NEEDS_PK_NOTE)),
        },
        SqlOp::Create | SqlOp::Update | SqlOp::Delete => {
            return Ok(QueryPlan::note(EXPLAIN_READ_ONLY_NOTE));
        }
    };

    // `AssertSqlSafe` (sqlx 0.9, #3723): `sql` here is one of `super::sql`'s
    // builder outputs, prefixed with a literal — no user data is interpolated,
    // the cursor still travels as the `$1` bind below.
    let explained = format!("EXPLAIN (COSTS true, FORMAT TEXT) {sql}");
    // `bind` is `Option<String>` for both arms on purpose: the list
    // query's cursor slot is genuinely NULL on the first page, and the
    // SQL casts it (`$1::text`) so Postgres can infer the type either
    // way.
    let rows: Vec<PgRow> = sqlx_core::query::query(sqlx_core::sql_str::AssertSqlSafe(explained))
        .bind(bind)
        .fetch_all(pool)
        .await?;

    Ok(collect_plan(rows))
}

fn collect_plan(rows: Vec<PgRow>) -> QueryPlan {
    let lines: Vec<String> = rows
        .into_iter()
        .filter_map(|r| r.try_get::<String, _>(0).ok())
        .collect();
    if lines.is_empty() {
        // Shouldn't happen — EXPLAIN always emits at least the top
        // plan node — but an empty plan is more usefully reported as a
        // note than as an empty code block.
        return QueryPlan::note("Postgres returned an empty plan.");
    }
    QueryPlan::text(lines.join("\n"))
}
