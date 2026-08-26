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
//!
//! # Actionable error for the `42703` case (cratestack#741 finding 2 follow-up)
//!
//! When the predicate references a column absent from `insert_values`
//! (typically a `@default(...)` column — see [`try_incoming_row_satisfies_predicate`]'s
//! doc comment), the derived-table `SELECT` above fails with Postgres
//! `42703 column "..." does not exist`. [`incoming_row_satisfies_predicate`]
//! special-cases exactly that SQLSTATE, from exactly this one query, into a
//! [`CratestackError::Validation`] naming the predicate and the likely
//! cause/workaround — not a generic [`cratestack_error_from_sqlx`] mapping
//! (which would leave the caller staring at a raw `DatabaseTyped` 500 with
//! no indication of what to change). This is deliberately narrow: only this
//! probe query, only `42703`; every other SQLSTATE, and every other call
//! site in this crate, keeps using `cratestack_error_from_sqlx` unchanged —
//! broadening that function itself would change unrelated error surfaces
//! that have nothing to do with this probe.

use cratestack_core::CratestackError;

use crate::query::support::push_bind_value;
use crate::sqlx::Acquire;
use crate::{SqlColumnValue, cratestack_error_from_sqlx, sqlx};

/// Postgres SQLSTATE for "undefined column".
const UNDEFINED_COLUMN_SQLSTATE: &str = "42703";

/// Maps an error from [`incoming_row_satisfies_predicate`]'s own probe
/// query. Narrowly special-cases `42703` (see the module doc comment);
/// every other error — including every other SQLSTATE, and any
/// non-`Database` `sqlx::Error` (finding 1's `ColumnDecode` case is
/// already handled by the `Option<bool>` decode above and never reaches
/// here) — falls through to the ordinary [`cratestack_error_from_sqlx`].
fn probe_evaluation_error(predicate: &'static str, error: sqlx::Error) -> CratestackError {
    if let sqlx::Error::Database(ref db_err) = error
        && db_err.code().as_deref() == Some(UNDEFINED_COLUMN_SQLSTATE)
    {
        return CratestackError::Validation(format!(
            "upsert conflict predicate `{predicate}` references a column that is not present \
             in the insert values, so it could not be evaluated before the row exists (Postgres: \
             {detail}). This usually means that column carries `@default(...)` in the schema — \
             any `@default(...)`, not just an `auth()`-derived one, excludes the field from the \
             generated create input, so the database's own column DEFAULT fills it and this \
             predicate can never see its value client-side. Fix by either supplying the column \
             explicitly in the input, or writing a predicate that only references columns the \
             input always carries.",
            detail = db_err.message(),
        ));
    }
    cratestack_error_from_sqlx(error)
}

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
        .map_err(|error| probe_evaluation_error(predicate, error))?;
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
///
/// Note this means the `42703`-specific [`CratestackError::Validation`]
/// [`probe_evaluation_error`] produces (cratestack#741 finding 2
/// follow-up) can never actually reach a caller through THIS function —
/// any probe error, `42703` or otherwise, is caught by the `match` below
/// and turned into `Ok(None)`. That error only ever surfaces through
/// [`incoming_row_satisfies_predicate`]'s OTHER caller, the DO UPDATE
/// path (`upsert_exec::run_upsert_in_tx`), which has no savepoint
/// fallback to swallow it — see that function's doc comment for why.
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
