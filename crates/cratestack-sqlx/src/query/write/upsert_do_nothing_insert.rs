//! [`run_insert_branch`] — the DO NOTHING path's authoritative insert
//! statement and race fallback (cratestack#487 / cratestack#741). Split
//! out of `upsert_do_nothing_exec.rs` purely to stay under this
//! codebase's ~200-LoC-per-file convention, not a behavioral boundary.

use cratestack_core::{
    AuditEvent, AuditOperation, CratestackContext, CratestackError, ModelEventKind,
};

use crate::audit::{build_audit_event, enqueue_audit_event};
use crate::descriptor::enqueue_event_outbox;
use crate::{ConflictTarget, ModelDescriptor, SqlColumnValue, SqlValue, SqlxRuntime, sqlx};

use super::upsert_do_nothing_authorize::authorize_existing_row;
use super::upsert_do_nothing_sql::upsert_returning_record_do_nothing;
use super::upsert_outcome::UpsertOutcome;
use super::upsert_sql::select_for_update_by_conflict_target;

/// `ON CONFLICT DO NOTHING RETURNING` is still the statement that runs
/// here — not a plain INSERT — because `resolve_pre_probe`'s "no row"
/// answer does not lock anything: a concurrent transaction can commit a
/// conflicting row in the gap between that SELECT and this INSERT. The
/// ON CONFLICT clause is the actual race guard; the pre-probe only
/// exists to pick Inserted vs. the (rare) fallback-read path below
/// without an extra round trip in the common case.
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_insert_branch<'tx, M, PK>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    insert_values: &[SqlColumnValue],
    conflict_target: ConflictTarget,
    conflict_columns: &[(&'static str, SqlValue)],
    ctx: &CratestackContext,
    emits_created: bool,
    audit_enabled: bool,
) -> Result<(UpsertOutcome<M>, bool, Option<AuditEvent>), CratestackError>
where
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
{
    match upsert_returning_record_do_nothing(&mut **tx, descriptor, insert_values, conflict_target)
        .await?
    {
        Some(record) => {
            if emits_created {
                enqueue_event_outbox(
                    &mut **tx,
                    descriptor.schema_name,
                    ModelEventKind::Created,
                    &record,
                )
                .await?;
            }
            let mut audit_event = None;
            if audit_enabled {
                let after = serde_json::to_value(&record).ok();
                let event = build_audit_event(descriptor, AuditOperation::Create, None, after, ctx);
                enqueue_audit_event(&mut **tx, &event).await?;
                audit_event = Some(event);
            }
            Ok((UpsertOutcome::Inserted(record), emits_created, audit_event))
        }
        None => {
            // Lost the race: another transaction committed a
            // conflicting row after our probe but before our INSERT.
            // Read it back under the same row lock the DO UPDATE path
            // uses (this also blocks until the winning transaction
            // commits, so we read its final data rather than an
            // in-flight write), so the caller gets the row that
            // actually exists rather than the stale values we
            // attempted to insert.
            //
            // If THAT row is deleted between our failed INSERT and
            // this SELECT — a second, narrower race on top of the
            // first — we do not invent a result: there is no row to
            // report as `Existing` and no insert succeeded either, so
            // this surfaces `CratestackError::Conflict` and leaves retrying
            // to the caller. Same predicate as the pre-probe above
            // (cratestack#741) — this fallback read must apply it too,
            // or it can read back a row the partial index doesn't
            // cover.
            let existing = select_for_update_by_conflict_target(
                &mut **tx,
                descriptor,
                conflict_columns,
                conflict_target.predicate(),
            )
            .await?
            .ok_or_else(|| {
                CratestackError::Conflict(format!(
                    "upsert do_nothing on `{}` lost a conflict race and the \
                         conflicting row was deleted before it could be read back; retry the call",
                    descriptor.table_name,
                ))
            })?;
            authorize_existing_row(
                runtime,
                descriptor,
                conflict_columns,
                conflict_target.predicate(),
                ctx,
            )
            .await?;
            Ok((UpsertOutcome::Existing(existing), false, None))
        }
    }
}
