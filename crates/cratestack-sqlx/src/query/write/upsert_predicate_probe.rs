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
//! Split across three files purely to stay under this codebase's
//! ~200-LoC-per-file convention, not a behavioral boundary:
//! this file builds and runs the probe query; [`upsert_predicate_probe_error`]
//! classifies what it can fail with; [`upsert_predicate_probe_savepoint`]
//! wraps it for the DO NOTHING path's fallback.
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
use crate::{SqlColumnValue, sqlx};

use super::upsert_predicate_probe_error::{ProbeOutcome, classify_probe_error};

/// `true` when `predicate`, evaluated against `insert_values`' own
/// column bindings, holds — i.e. whether the row about to be inserted
/// would itself be a member of the partial index's domain. Callers
/// only need this before a *pre-probe* (deciding insert-vs-update
/// branching ahead of the real `ON CONFLICT` statement); once a real
/// conflict has already been confirmed by the database (the post-race
/// fallback read, or the update-policy check on a row already known to
/// conflict), this check is redundant — the database's own decision
/// already implies the incoming row satisfied the predicate.
///
/// Returns a [`ProbeOutcome`] on failure rather than a bare
/// `CratestackError` so `upsert_predicate_probe_savepoint`'s fallback
/// can discriminate `42703` from every other failure by construction
/// — see `upsert_predicate_probe_error`'s module doc comment.
pub(super) async fn incoming_row_satisfies_predicate_inner<'e, E>(
    executor: E,
    insert_values: &[SqlColumnValue],
    predicate: &'static str,
) -> Result<bool, ProbeOutcome>
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
        .map_err(|error| classify_probe_error(predicate, error))?;
    Ok(matches.unwrap_or(false))
}

/// [`incoming_row_satisfies_predicate_inner`], collapsed to a plain
/// `CratestackError` — used directly by the DO UPDATE path
/// (`upsert_exec::run_upsert_in_tx`), which has no savepoint fallback
/// and so propagates either [`ProbeOutcome`] class as an error
/// identically (both still carry the friendly `42703` message when
/// applicable — only the "swallow and fall back" behavior is DO
/// NOTHING-specific, not the error message itself; see that function's
/// doc comment for why DO UPDATE can't safely swallow `42703` the way
/// the DO NOTHING path does).
pub(super) async fn incoming_row_satisfies_predicate<'e, E>(
    executor: E,
    insert_values: &[SqlColumnValue],
    predicate: &'static str,
) -> Result<bool, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    incoming_row_satisfies_predicate_inner(executor, insert_values, predicate)
        .await
        .map_err(ProbeOutcome::into_error)
}
