//! Core upsert body for `.do_nothing()` (cratestack#487): probe the
//! conflict target under the same row lock the DO UPDATE path
//! (`upsert_exec::run_upsert_in_tx`) uses, but never issue a mutating
//! statement against a row that already exists — either hand back the
//! already-locked probe result directly, or (insert branch) issue a
//! genuine `ON CONFLICT DO NOTHING` and fall back to a locked read if
//! a concurrent transaction won the race. See `UpsertOutcome`'s doc
//! comment for the full race-semantics writeup this implements.

use cratestack_core::{
    AuditEvent, AuditOperation, CratestackContext, CratestackError, ModelEventKind,
};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::evaluate_create_policies;
use crate::{ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, sqlx};

use super::upsert_do_nothing_authorize::authorize_existing_row;
use super::upsert_do_nothing_sql::upsert_returning_record_do_nothing;
use super::upsert_outcome::UpsertOutcome;
use super::upsert_predicate_probe::try_incoming_row_satisfies_predicate;
use super::upsert_prepare::prepare_upsert_insert;
use super::upsert_sql::select_for_update_by_conflict_target;

/// Returns `(outcome, emits_any_event, audit_event)` — same contract
/// as `upsert_exec::run_upsert_in_tx`: the caller decides whether to
/// drain the outbox / fan the audit event out, both only after it
/// commits `tx`.
pub(super) async fn run_upsert_do_nothing_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    conflict_target: ConflictTarget,
    ctx: &CratestackContext,
) -> Result<(UpsertOutcome<M>, bool, Option<AuditEvent>), CratestackError>
where
    I: UpsertModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;
    let (insert_values, conflict_columns) =
        prepare_upsert_insert(descriptor, &input, ctx, conflict_target)?;

    // Same create-policy gate as `run_upsert_in_tx`: `.do_nothing()`
    // still performs a real INSERT on the insert branch, so create
    // policy applies unconditionally, same as `.create()` / the DO
    // UPDATE upsert.
    if !evaluate_create_policies(
        runtime.pool(),
        descriptor.create_allow_policies,
        descriptor.create_deny_policies,
        &insert_values,
        ctx,
    )
    .await?
    {
        return Err(CratestackError::Forbidden(
            "create policy denied this upsert".to_owned(),
        ));
    }

    let emits_created = descriptor.emits(ModelEventKind::Created);
    let audit_enabled = descriptor.audit_enabled;
    if emits_created {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(runtime).await?;
    }

    // Probe under a row lock, exactly like the DO UPDATE path. If a
    // row is already there, holding that lock for the rest of this
    // transaction guarantees it is still there when we commit — DO
    // NOTHING semantics are then just "return what the probe found",
    // no second statement required.
    //
    // The predicate travels with this pre-probe in two distinct ways
    // (cratestack#741), both required — see `upsert_exec::run_upsert_in_tx`'s
    // matching comment for the full reasoning: filtering candidate
    // existing rows by the predicate alone is not sufficient, because
    // an incoming row that doesn't itself satisfy the predicate can
    // never conflict via a partial index no matter what else exists.
    //
    // `try_incoming_row_satisfies_predicate` can itself fail to
    // evaluate the predicate (cratestack#741 finding 2) — most
    // commonly because the predicate references a column a
    // `@default(...)` schema attribute excludes from `insert_values`
    // (the database's own column DEFAULT fills it, so this crate never
    // learns its value client-side, and the one-row derived table the
    // check builds has no such column: Postgres raises `42703 column
    // "..." does not exist`). This is NOT propagated as a 500 here:
    // unlike the DO UPDATE path (see `run_upsert_in_tx`'s matching
    // comment for why that one is different), DO NOTHING's real `ON
    // CONFLICT ... DO NOTHING RETURNING` statement below is
    // unconditionally the authoritative race guard and decides
    // Inserted-vs-Existing correctly on its own — the pre-probe exists
    // purely to avoid an extra round trip in the common case (see this
    // function's module doc comment). So when the pre-probe can't be
    // evaluated, the safe move is simply to skip it entirely and fall
    // straight through to that authoritative statement, rather than
    // guess. (`try_incoming_row_satisfies_predicate` runs the check in
    // its own SAVEPOINT precisely so a failed check doesn't poison the
    // rest of `tx` before we get to that authoritative statement.)
    let pre_probe = match conflict_target.predicate() {
        Some(predicate) => {
            match try_incoming_row_satisfies_predicate(tx, &insert_values, predicate).await? {
                Some(true) => {
                    select_for_update_by_conflict_target(
                        &mut **tx,
                        descriptor,
                        &conflict_columns,
                        Some(predicate),
                    )
                    .await?
                }
                Some(false) | None => None,
            }
        }
        None => {
            select_for_update_by_conflict_target(&mut **tx, descriptor, &conflict_columns, None)
                .await?
        }
    };
    if let Some(existing) = pre_probe {
        authorize_existing_row(
            runtime,
            descriptor,
            &conflict_columns,
            conflict_target.predicate(),
            ctx,
        )
        .await?;
        // No SQL runs against the row: DO NOTHING never touches it, so
        // there is nothing to audit and no event to emit — the record
        // genuinely did not change.
        return Ok((UpsertOutcome::Existing(existing), false, None));
    }

    // Insert branch. `ON CONFLICT DO NOTHING RETURNING` is still the
    // statement that runs here — not a plain INSERT — because the
    // probe's "no row" answer above does not lock anything: a
    // concurrent transaction can commit a conflicting row in the gap
    // between our SELECT and this INSERT. The ON CONFLICT clause is
    // the actual race guard; the probe only exists to pick Inserted
    // vs. the (rare) fallback-read path below without an extra round
    // trip in the common case.
    match upsert_returning_record_do_nothing(&mut **tx, descriptor, &insert_values, conflict_target)
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
                &conflict_columns,
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
                &conflict_columns,
                conflict_target.predicate(),
                ctx,
            )
            .await?;
            Ok((UpsertOutcome::Existing(existing), false, None))
        }
    }
}
