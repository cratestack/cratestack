//! [`try_incoming_row_satisfies_predicate`] — the DO NOTHING path's
//! savepoint-wrapped, narrowly-falling-back variant of
//! [`super::upsert_predicate_probe::incoming_row_satisfies_predicate`]
//! (cratestack#741 finding 2). Split out of `upsert_predicate_probe.rs`
//! purely to stay under this codebase's ~200-LoC-per-file convention,
//! not a behavioral boundary.

use cratestack_core::CratestackError;

use crate::sqlx::Acquire;
use crate::{SqlColumnValue, cratestack_error_from_sqlx, sqlx};

use super::upsert_predicate_probe::incoming_row_satisfies_predicate_inner;
use super::upsert_predicate_probe_error::ProbeOutcome;

/// [`super::upsert_predicate_probe::incoming_row_satisfies_predicate`],
/// but wrapped in its own `SAVEPOINT` so a probe-evaluation failure
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
/// Returns `Ok(None)` only for [`ProbeOutcome::UndefinedColumn`]
/// (savepoint rolled back) — callers treat this the same as "unknown,
/// fall back to the authoritative statement". Every other failure —
/// [`ProbeOutcome::Other`], or a failure of the savepoint machinery
/// itself (`BEGIN`/`COMMIT`/`ROLLBACK`, not the probe query) —
/// propagates as `Err` rather than being absorbed: a masked connection
/// loss, statement timeout, or permission error would otherwise
/// resurface later as a different, more confusing failure from a
/// different statement (or, worse, silently succeed if the later
/// statement happens not to need whatever failed). This is narrow BY
/// CONSTRUCTION — matching on [`ProbeOutcome`]'s variant, not
/// inspecting the mapped error's shape — see
/// `upsert_predicate_probe_error`'s module doc comment.
pub(super) async fn try_incoming_row_satisfies_predicate<'tx>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    insert_values: &[SqlColumnValue],
    predicate: &'static str,
) -> Result<Option<bool>, CratestackError> {
    let mut savepoint = tx.begin().await.map_err(cratestack_error_from_sqlx)?;
    match incoming_row_satisfies_predicate_inner(&mut *savepoint, insert_values, predicate).await {
        Ok(satisfies) => {
            savepoint
                .commit()
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(Some(satisfies))
        }
        Err(ProbeOutcome::UndefinedColumn(_)) => {
            savepoint
                .rollback()
                .await
                .map_err(cratestack_error_from_sqlx)?;
            Ok(None)
        }
        Err(ProbeOutcome::Other(error)) => {
            // Best-effort cleanup: if `error` itself was a connection
            // loss, this rollback likely fails too, but `error` is what
            // matters to the caller either way — `run()`/`run_in_tx()`
            // never commit `tx` on an error path, so it gets rolled
            // back in full regardless of whether this savepoint-level
            // rollback succeeds.
            let _ = savepoint.rollback().await;
            Err(error)
        }
    }
}
