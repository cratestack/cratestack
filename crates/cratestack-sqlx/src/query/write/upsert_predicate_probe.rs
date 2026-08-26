//! Whether the row about to be inserted would itself fall within a
//! partial unique index's predicate (cratestack#741).
//!
//! This is the half of the conflict-probe fix that isn't just "filter
//! candidate existing rows by the predicate too" (that alone is what
//! `upsert_sql::select_for_update_by_conflict_target` does). Postgres's
//! own partial-index semantics: a row is only ever added to a partial
//! index's B-tree if *that row itself* satisfies the index predicate.
//! An incoming row that does NOT satisfy the predicate therefore can
//! **never** conflict via that index, no matter what else already
//! exists in the table — so a probe that only checks "is there a
//! matching existing row within the predicate" is still wrong on its
//! own: it can find a genuinely unrelated existing row (one the
//! partial index *does* cover) and wrongly report a conflict for an
//! incoming row the index doesn't cover at all. Both halves — this
//! check and the existing-row predicate filter — are required together.
//!
//! The incoming row's own values are re-bound into a one-row derived
//! table (`SELECT $1 AS col1, $2 AS col2, ... `) and the predicate is
//! evaluated against it server-side, rather than attempting to
//! interpret the (opaque, `&'static str`) predicate client-side — the
//! predicate can be arbitrary SQL, so only Postgres itself can
//! evaluate it correctly.
//!
//! # Three-valued logic (cratestack#741 finding 1)
//!
//! SQL predicates are three-valued, not boolean: `status = 'active'`
//! evaluates to `NULL`, not `false`, whenever `status` is `NULL`.
//! Decoding `SELECT (<predicate>)` as `(bool,)` therefore fails with a
//! `sqlx::Error::ColumnDecode` (`UnexpectedNullError`) the moment the predicate
//! touches a NULL column — which surfaces to the caller as an opaque
//! 500 (`cratestack_error_from_sqlx` has no dedicated arm for a decode
//! error, so it falls through to `CratestackError::Database`). This
//! module decodes `(Option<bool>,)` instead and treats `NULL` the same
//! as `false`: Postgres's own partial-index semantics only ever admit
//! a row to the index whose predicate evaluates to `true` (`UNKNOWN`,
//! same as `false` here, keeps a row out of the index), so a `NULL`
//! predicate result means the incoming row is — like a `false` result
//! — outside the index's domain and therefore cannot conflict via it.

use cratestack_core::CratestackError;

use crate::query::support::push_bind_value;
use crate::sqlx::Acquire;
use crate::{SqlColumnValue, cratestack_error_from_sqlx, sqlx};

/// `true` when `predicate`, evaluated against `insert_values`' own
/// column bindings, holds — i.e. whether the row about to be inserted
/// would itself be a member of the partial index's domain. Callers
/// only need this before a *pre-probe* (deciding insert-vs-update
/// branching ahead of the real `ON CONFLICT` statement); once a real
/// conflict has already been confirmed by the database (the post-race
/// fallback read, or the update-policy check on a row already known to
/// conflict), this check is redundant — the database's own decision
/// already implies the incoming row satisfied the predicate.
pub(super) async fn incoming_row_satisfies_predicate<'e, E>(
    executor: E,
    insert_values: &[SqlColumnValue],
    predicate: &'static str,
) -> Result<bool, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT (");
    query.push(predicate);
    query.push(") FROM (SELECT ");
    for (idx, value) in insert_values.iter().enumerate() {
        if idx > 0 {
            query.push(", ");
        }
        push_bind_value(&mut query, &value.value);
        query.push(" AS ").push(value.column);
    }
    query.push(") AS incoming_row");

    // `Option<bool>` — see the module doc comment: a `NULL` predicate
    // result (three-valued SQL logic) is treated the same as `false`,
    // not as a decode error.
    let (matches,): (Option<bool>,) = query
        .build_query_as::<(Option<bool>,)>()
        .fetch_one(executor)
        .await
        .map_err(cratestack_error_from_sqlx)?;
    Ok(matches.unwrap_or(false))
}

/// [`incoming_row_satisfies_predicate`], but wrapped in its own
/// `SAVEPOINT` so a probe-evaluation failure (cratestack#741 finding
/// 2 — most commonly the predicate referencing a column a
/// `@default(...)` schema attribute excludes from `insert_values`)
/// doesn't poison the rest of `tx`.
///
/// Postgres aborts an entire transaction the instant any statement in
/// it errors: every subsequent statement on that same transaction then
/// fails with `25P02 current transaction is aborted, commands ignored
/// until end of transaction block`, even an unrelated one, until a
/// `ROLLBACK` (or `ROLLBACK TO SAVEPOINT`) runs. `run_upsert_do_nothing_in_tx`
/// needs to keep issuing statements against `tx` after a probe failure
/// (the real `ON CONFLICT ... DO NOTHING RETURNING`, which is
/// authoritative regardless — see that function's doc comment), so the
/// probe has to run inside its own savepoint that gets rolled back on
/// failure, leaving the outer transaction clean. Confirmed live: without
/// this savepoint, the very first upsert against such a schema fails
/// with `25P02` instead of the intended fallback behavior, because the
/// probe's own `42703` had already aborted `tx`.
///
/// Returns `Ok(None)` when the probe itself could not be evaluated
/// (savepoint rolled back) — callers treat this the same as "unknown,
/// fall back to the authoritative statement". Only a failure of the
/// savepoint machinery itself (`BEGIN`/`COMMIT`/`ROLLBACK`, not the
/// probe query) propagates as `Err`.
pub(super) async fn try_incoming_row_satisfies_predicate<'tx>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    insert_values: &[SqlColumnValue],
    predicate: &'static str,
) -> Result<Option<bool>, CratestackError> {
    let mut savepoint = tx.begin().await.map_err(cratestack_error_from_sqlx)?;
    match incoming_row_satisfies_predicate(&mut *savepoint, insert_values, predicate).await {
        Ok(satisfies) => {
            savepoint
                .commit()
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(Some(satisfies))
        }
        Err(_probe_evaluation_failed) => {
            savepoint
                .rollback()
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(None)
        }
    }
}
