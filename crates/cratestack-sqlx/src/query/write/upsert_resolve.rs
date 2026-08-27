//! Insert-vs-update resolution for the `.upsert(..).run(..)` (`ON
//! CONFLICT ... DO UPDATE`) path — cratestack#745.
//!
//! The pre-lock probe in `upsert_exec::run_upsert_in_tx` is a
//! *prediction*, and it is only ever binding in one direction. When it
//! finds a row it has already locked it with `SELECT ... FOR UPDATE`, so
//! nothing can delete that row before the statement runs: "update" is a
//! guarantee. When it finds nothing it has locked nothing — a concurrent
//! transaction can commit a conflicting row in the gap, and the real
//! statement then performs a genuine UPDATE while the runtime still
//! believes it inserted. That mislabelling is cratestack#745: a
//! `Created` event and an `AuditOperation::Create` with no
//! before-snapshot describing what was actually an update.
//!
//! # Why `DO NOTHING` first rather than `RETURNING (xmax = 0)`
//!
//! cratestack#745 proposed `RETURNING (xmax = 0) AS inserted`, which
//! classifies correctly but only tells us *that* we lost — by the time
//! it answers, the row the loser needed for its audit before-snapshot
//! has already been overwritten, and Postgres exposes no way to read the
//! superseded tuple version back. It is also a storage-layer
//! implementation detail with no equivalent in any other engine.
//!
//! So the insert branch instead runs the statement `.do_nothing()`
//! already relies on (`upsert_do_nothing_insert::run_insert_branch` is
//! the reference shape the ticket points at): `INSERT ... ON CONFLICT
//! (<target>) DO NOTHING RETURNING ...`. Postgres only RETURNs rows a
//! statement touched, so "a row came back" *is* the database's own
//! answer to "did I insert?" — documented behaviour, not an internal
//! detail. On a conflict it returns nothing **without having touched the
//! winning row**, which leaves the loser able to do the whole update
//! branch properly: lock the winner, re-run the update policy gate that
//! the mispredicted branch skipped entirely, capture a real
//! before-snapshot, and only then issue the DO UPDATE statement.
//!
//! This also keeps the discriminator in the runtime rather than in the
//! SQL, which is the property the old comment in `upsert_exec.rs`
//! defended `xmax` avoidance for ("makes the rusqlite mirror trivial").
//! `ON CONFLICT DO NOTHING ... RETURNING` is SQLite 3.35+ surface too,
//! so an embedded mirror of this sequencing stays a transliteration;
//! `xmax` would have had no SQLite counterpart at all.
//!
//! Off the race path nothing observable changes: an uncontended insert
//! is still one statement returning the inserted row, and the update
//! branch (probe found and locked a row) runs exactly the SQL it always
//! did.

use cratestack_core::{CratestackContext, CratestackError};

use crate::{ConflictTarget, ModelDescriptor, SqlColumnValue, SqlValue, SqlxRuntime, sqlx};

use super::upsert_do_nothing_sql::upsert_returning_record_do_nothing;
use super::upsert_do_update_sql::upsert_returning_record;
use super::upsert_sql::{row_passes_update_policy, select_for_update_by_conflict_target};

/// What the statement actually did, as opposed to what the probe
/// predicted. `before` is `Some` exactly when [`Self::inserted`] is
/// `false` **and** the prior row was readable — see
/// [`resolve_upsert`]'s last branch for the one case where an update is
/// reported without one.
pub(super) struct UpsertResolution<M> {
    pub(super) record: M,
    pub(super) inserted: bool,
    pub(super) before: Option<M>,
}

/// Run the conflict-bearing statement and report what it did.
///
/// `before_record` is the pre-lock probe's answer: `Some` means the
/// probe found *and locked* a row, `None` means it found none.
#[allow(clippy::too_many_arguments)]
pub(super) async fn resolve_upsert<'tx, M, PK>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
    conflict_columns: &[(&'static str, SqlValue)],
    ctx: &CratestackContext,
    before_record: Option<M>,
) -> Result<UpsertResolution<M>, CratestackError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
{
    // Update branch. The probe holds a row lock, so no concurrent
    // transaction can turn this into an insert; the prediction is a
    // guarantee and the SQL below is byte-identical to pre-#745.
    if let Some(before) = before_record {
        gate_update_policy(runtime, descriptor, conflict_columns, conflict_target, ctx).await?;
        let record =
            upsert_returning_record(&mut **tx, descriptor, insert_values, conflict_target).await?;
        return Ok(UpsertResolution {
            record,
            inserted: false,
            before: Some(before),
        });
    }

    // Insert branch. `DO NOTHING` rather than `DO UPDATE` so that a lost
    // race is *reported* instead of silently performed: on conflict this
    // returns no row and leaves the winner untouched, so the recovery
    // below can still observe the prior state. On the uncontended path
    // (the overwhelmingly common one) it inserts and returns the row in
    // a single statement, exactly as `DO UPDATE` did.
    if let Some(record) =
        upsert_returning_record_do_nothing(&mut **tx, descriptor, insert_values, conflict_target)
            .await?
    {
        return Ok(UpsertResolution {
            record,
            inserted: true,
            before: None,
        });
    }

    // Lost the race. Re-enter the update branch from the top, with the
    // same probe the caller ran — it blocks until the winning
    // transaction commits, so it reads that transaction's final data,
    // and the lock it takes means the `DO UPDATE` below is now
    // guaranteed to update rather than insert.
    let before = select_for_update_by_conflict_target(
        &mut **tx,
        descriptor,
        conflict_columns,
        conflict_target.predicate(),
    )
    .await?;
    if before.is_some() {
        gate_update_policy(runtime, descriptor, conflict_columns, conflict_target, ctx).await?;
    }
    let record =
        upsert_returning_record(&mut **tx, descriptor, insert_values, conflict_target).await?;

    // `before.is_none()` here means the conflict is real (the DO NOTHING
    // above declined to insert) but invisible to the probe. Two known
    // causes, and reporting "inserted" is what this path did before
    // cratestack#745 in both, so it is preserved rather than quietly
    // changed:
    //   * the winning row was deleted again between the two statements —
    //     in which case the `DO UPDATE` above really did insert and
    //     "inserted" is the correct answer;
    //   * a soft-delete tombstone sits at the conflict target.
    //     `select_for_update_by_conflict_target` deliberately treats
    //     tombstones as "no row", so the DO UPDATE revives one and calls
    //     it a create. That is a *separate* pre-existing defect (the
    //     `.do_nothing()` path surfaces `Conflict` for the same shape);
    //     fixing it here would change behaviour off the race path, which
    //     cratestack#745 explicitly rules out.
    let inserted = before.is_none();
    Ok(UpsertResolution {
        record,
        inserted,
        before,
    })
}

/// The update-policy gate, shared by the predicted and the recovered
/// update branch. Recovering the race without it would let a caller
/// update a row it has no `update` permission on purely because a
/// concurrent commit landed at the wrong moment.
async fn gate_update_policy<M, PK>(
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    conflict_columns: &[(&'static str, SqlValue)],
    conflict_target: ConflictTarget,
    ctx: &CratestackContext,
) -> Result<(), CratestackError> {
    if row_passes_update_policy(
        runtime.pool(),
        descriptor,
        conflict_columns,
        conflict_target.predicate(),
        ctx,
    )
    .await?
    {
        return Ok(());
    }
    Err(CratestackError::Forbidden(
        "update policy denied this upsert".to_owned(),
    ))
}
