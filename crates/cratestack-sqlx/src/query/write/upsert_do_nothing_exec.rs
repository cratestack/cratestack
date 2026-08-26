//! Core upsert body for `.do_nothing()` (cratestack#487): probe the
//! conflict target under the same row lock the DO UPDATE path
//! (`upsert_exec::run_upsert_in_tx`) uses, but never issue a mutating
//! statement against a row that already exists — either hand back the
//! already-locked probe result directly, or (insert branch) issue a
//! genuine `ON CONFLICT DO NOTHING` and fall back to a locked read if
//! a concurrent transaction won the race. See `UpsertOutcome`'s doc
//! comment for the full race-semantics writeup this implements.
//!
//! The two branches — deciding which one to take
//! (`upsert_do_nothing_probe::resolve_pre_probe`) and running the
//! authoritative insert statement
//! (`upsert_do_nothing_insert::run_insert_branch`) — are split into
//! their own files purely to stay under this codebase's
//! ~200-LoC-per-file convention, not a behavioral boundary; this file
//! is left as the orchestration: policy gates, event/audit table
//! setup, and dispatch between the two.

use cratestack_core::{AuditEvent, CratestackContext, CratestackError, ModelEventKind};

use crate::audit::ensure_audit_table;
use crate::descriptor::ensure_event_outbox_table;
use crate::query::support::evaluate_create_policies;
use crate::{ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, sqlx};

use super::upsert_do_nothing_authorize::authorize_existing_row;
use super::upsert_do_nothing_insert::run_insert_branch;
use super::upsert_do_nothing_probe::resolve_pre_probe;
use super::upsert_outcome::UpsertOutcome;
use super::upsert_prepare::prepare_upsert_insert;

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

    let pre_probe = resolve_pre_probe(
        tx,
        descriptor,
        &conflict_columns,
        &insert_values,
        conflict_target,
    )
    .await?;
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

    run_insert_branch(
        tx,
        runtime,
        descriptor,
        &insert_values,
        conflict_target,
        &conflict_columns,
        ctx,
        emits_created,
        audit_enabled,
    )
    .await
}
