//! [`resolve_pre_probe`] — the DO NOTHING path's pre-probe branch
//! decision (cratestack#487 / cratestack#741). Split out of
//! `upsert_do_nothing_exec.rs` purely to stay under this codebase's
//! ~200-LoC-per-file convention, not a behavioral boundary.

use cratestack_core::CratestackError;

use crate::{ConflictTarget, ModelDescriptor, SqlValue, sqlx};

use super::upsert_predicate_probe_savepoint::try_incoming_row_satisfies_predicate;
use super::upsert_sql::select_for_update_by_conflict_target;

/// Probe under a row lock, exactly like the DO UPDATE path
/// (`upsert_exec::run_upsert_in_tx`). If a row is already there,
/// holding that lock for the rest of this transaction guarantees it is
/// still there when the caller commits — DO NOTHING semantics are then
/// just "return what the probe found", no second statement required.
/// `None` means the insert branch:
/// `run_upsert_do_nothing_in_tx` issues the real
/// `ON CONFLICT DO NOTHING RETURNING` next.
///
/// The predicate travels with this pre-probe in two distinct ways
/// (cratestack#741), both required — see `upsert_exec::run_upsert_in_tx`'s
/// matching comment for the full reasoning: filtering candidate
/// existing rows by the predicate alone is not sufficient, because an
/// incoming row that doesn't itself satisfy the predicate can never
/// conflict via a partial index no matter what else exists.
///
/// `try_incoming_row_satisfies_predicate` can itself fail to evaluate
/// the predicate (cratestack#741 finding 2) — most commonly because the
/// predicate references a column a `@default(...)` schema attribute
/// excludes from `insert_values` (the database's own column DEFAULT
/// fills it, so this crate never learns its value client-side, and the
/// one-row derived table the check builds has no such column: Postgres
/// raises `42703 column "..." does not exist`). That specific failure
/// (and ONLY that one — see `upsert_predicate_probe_error`'s module doc
/// comment for why nothing else is treated this way) is NOT propagated
/// as a 500 here: unlike the DO UPDATE path (see `run_upsert_in_tx`'s
/// matching comment for why that one is different), DO NOTHING's real
/// `ON CONFLICT ... DO NOTHING RETURNING` statement
/// (`upsert_do_nothing_insert::run_insert_branch`) is unconditionally
/// the authoritative race guard and decides Inserted-vs-Existing
/// correctly on its own — the pre-probe exists purely to avoid an extra
/// round trip in the common case (see `upsert_do_nothing_exec`'s module
/// doc comment). So when the pre-probe can't be evaluated for that one
/// reason, the safe move is simply to skip it entirely and fall
/// straight through to that authoritative statement, rather than guess.
/// Every OTHER probe failure still propagates normally through the `?`
/// below. (`try_incoming_row_satisfies_predicate` runs the check in its
/// own SAVEPOINT precisely so a failed check doesn't poison the rest of
/// `tx` before we get to that authoritative statement.)
pub(super) async fn resolve_pre_probe<'tx, M, PK>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict_columns: &[(&'static str, SqlValue)],
    insert_values: &[crate::SqlColumnValue],
    conflict_target: ConflictTarget,
) -> Result<Option<M>, CratestackError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    match conflict_target.predicate() {
        Some(predicate) => {
            match try_incoming_row_satisfies_predicate(tx, insert_values, predicate).await? {
                Some(true) => {
                    select_for_update_by_conflict_target(
                        &mut **tx,
                        descriptor,
                        conflict_columns,
                        Some(predicate),
                    )
                    .await
                }
                Some(false) | None => Ok(None),
            }
        }
        None => {
            select_for_update_by_conflict_target(&mut **tx, descriptor, conflict_columns, None)
                .await
        }
    }
}
