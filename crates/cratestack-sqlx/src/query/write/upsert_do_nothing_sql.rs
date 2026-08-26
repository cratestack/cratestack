//! SQL emitter for the `.do_nothing()` conflict path: the genuine
//! `INSERT ... ON CONFLICT (...) DO NOTHING RETURNING ...` — as
//! opposed to `upsert_sql::upsert_returning_record`'s `DO UPDATE`
//! (real or no-op-self-assignment). See the comment in `upsert_sql.rs`
//! for why these two stay separate mechanisms.

use cratestack_core::CratestackError;

use crate::query::support::{classify_unique_violation, push_bind_value};
use crate::{ConflictTarget, ModelDescriptor, SqlColumnValue, sqlx};

/// Render and execute `INSERT ... ON CONFLICT (<target>) DO NOTHING
/// RETURNING ...`. Returns `None` on conflict — Postgres never RETURNs
/// a row a `DO NOTHING` didn't touch, which is exactly why this
/// primitive can't stand alone: see
/// `upsert_do_nothing_exec::run_upsert_do_nothing_in_tx` for how the
/// caller resolves a `None` back into the actual conflicting row.
///
/// Unlike `upsert_sql::upsert_returning_record`, this never fires
/// `BEFORE`/`AFTER UPDATE` triggers or generates WAL for an existing
/// row — `DO NOTHING` genuinely does not touch it.
pub(super) async fn upsert_returning_record_do_nothing<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
) -> Result<Option<M>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO ");
    query.push(descriptor.table_name).push(" (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        query.push(value.column);
    }
    query.push(") VALUES (");
    for (index, value) in insert_values.iter().enumerate() {
        if index > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
    }

    query.push(") ON CONFLICT (");
    match conflict_target.as_columns() {
        None => {
            query.push(descriptor.primary_key);
        }
        Some(cols) => {
            for (idx, column) in cols.iter().enumerate() {
                if idx > 0 {
                    query.push(", ");
                }
                query.push(*column);
            }
        }
    }
    query.push(")");
    // Unpredicated targets emit byte-identical SQL to before cratestack#741 —
    // this branch is a no-op when `predicate()` is `None`.
    if let Some(predicate) = conflict_target.predicate() {
        query.push(" WHERE ").push(predicate);
    }
    query.push(" DO NOTHING RETURNING ");
    query.push(descriptor.select_projection());

    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(classify_unique_violation)
}
