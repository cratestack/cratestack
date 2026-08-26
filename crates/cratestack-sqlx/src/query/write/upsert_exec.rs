//! Core upsert body: probe the conflict target under a row lock, pick
//! the insert-vs-update branch, run the appropriate policy + audit +
//! outbox writes, then issue the conflict-bearing INSERT.

use cratestack_core::{
    AuditEvent, AuditOperation, CratestackContext, CratestackError, ModelEventKind,
};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::evaluate_create_policies;
use crate::{ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, sqlx};

use super::upsert_do_update_sql::upsert_returning_record;
use super::upsert_predicate_probe::incoming_row_satisfies_predicate;
use super::upsert_prepare::prepare_upsert_insert;
use super::upsert_sql::{row_passes_update_policy, select_for_update_by_conflict_target};

/// Returns `(record, emits_any_event, audit_event)` — the caller
/// decides whether to drain the outbox / fan the audit event out to
/// the installed `AuditSink`, both only after it commits `tx`.
pub(super) async fn run_upsert_in_tx<'tx, M, PK, I>(
    tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    runtime: &SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    input: I,
    conflict_target: ConflictTarget,
    ctx: &CratestackContext,
) -> Result<(M, bool, Option<AuditEvent>), CratestackError>
where
    I: UpsertModelInput<M>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    input.validate()?;
    let (insert_values, conflict_columns) =
        prepare_upsert_insert(descriptor, &input, ctx, conflict_target)?;

    // Both create and update policies must allow the call. Stricter
    // than "evaluate the path that runs," but pre-flighting a read
    // just to pick the policy slot would leak row existence.
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
    let emits_updated = descriptor.emits(ModelEventKind::Updated);
    let audit_enabled = descriptor.audit_enabled;

    if emits_created || emits_updated {
        ensure_event_outbox_table(&mut **tx).await?;
    }
    if audit_enabled {
        ensure_audit_table(runtime).await?;
    }

    // Probe the conflict target under a row-level lock. If a row
    // exists, this is the update branch; otherwise it's the insert
    // branch. The lock serializes concurrent upserts on the same key.
    //
    // The predicate travels with the probe in two distinct ways
    // (cratestack#741), both required:
    //   1. Filtering on the conflict columns alone can match an
    //      EXISTING row the partial index does not cover —
    //      `select_for_update_by_conflict_target`'s `predicate`
    //      argument handles this.
    //   2. Postgres only adds a row to a partial index's B-tree if
    //      THAT row itself satisfies the predicate — an incoming row
    //      that does not satisfy it can never conflict via that index,
    //      no matter what already exists. Skipping the probe entirely
    //      when the incoming row falls outside the predicate is what
    //      `incoming_row_satisfies_predicate` guards here; without it
    //      an out-of-predicate incoming row could still be told
    //      "conflicts with" some unrelated in-predicate existing row
    //      that happens to share the conflict columns.
    //
    // Unlike `upsert_do_nothing_exec::run_upsert_do_nothing_in_tx`,
    // this path does NOT fall back to "skip the pre-probe" when
    // `incoming_row_satisfies_predicate` itself fails to evaluate
    // (cratestack#741 finding 2 — e.g. the predicate references a
    // `@default(...)` column excluded from `insert_values`). DO
    // NOTHING can do that safely because its real `ON CONFLICT ...
    // DO NOTHING RETURNING` statement is unconditionally authoritative
    // for Inserted-vs-Existing on its own. This DO UPDATE path has no
    // equivalent authoritative fallback: `before_record` is the ONLY
    // signal that picks Created-vs-Updated, the audit before-snapshot,
    // and the update-policy gate — it is never reconciled against what
    // the real `ON CONFLICT ... DO UPDATE` statement actually did (see
    // `UpsertOutcome`'s doc comment / the tracked pre-existing race on
    // this same field). Silently treating an unevaluable predicate as
    // "no existing row" would deterministically mislabel every genuine
    // update on such a schema as a Create, on every call, not just
    // under a race — a worse, silent defect, not a fix. So the probe
    // failure is left to propagate as an error here rather than
    // guessed at; closing this gap for real needs either teaching
    // `insert_values` to backfill literal `@default(...)` values (a
    // codegen change reaching every create/insert path, not just
    // upsert) or basing Created-vs-Updated on the real statement's own
    // result instead of a pre-probe — both bigger than this fix.
    //
    // The specific, common case — the predicate references a column a
    // `@default(...)` schema attribute excludes from `insert_values` —
    // no longer surfaces as an opaque `DatabaseTyped` 500, though:
    // `incoming_row_satisfies_predicate` narrowly detects Postgres
    // `42703` from this exact query and maps it to a
    // `CratestackError::Validation` naming the predicate and the fix
    // (cratestack#741 finding 2 follow-up; see `upsert_predicate_probe_error.rs`).
    // Every other error from this call — including every other
    // SQLSTATE — still propagates as the ordinary `cratestack_error_from_sqlx`
    // mapping, unchanged.
    let before_record = match conflict_target.predicate() {
        Some(predicate)
            if !incoming_row_satisfies_predicate(&mut **tx, &insert_values, predicate).await? =>
        {
            None
        }
        predicate => {
            select_for_update_by_conflict_target(
                &mut **tx,
                descriptor,
                &conflict_columns,
                predicate,
            )
            .await?
        }
    };
    let inserted = before_record.is_none();

    if !inserted
        && !row_passes_update_policy(
            runtime.pool(),
            descriptor,
            &conflict_columns,
            conflict_target.predicate(),
            ctx,
        )
        .await?
    {
        return Err(CratestackError::Forbidden(
            "update policy denied this upsert".to_owned(),
        ));
    }

    let before_snapshot = if !inserted && audit_enabled {
        before_record
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok())
    } else {
        None
    };

    let record =
        upsert_returning_record(&mut **tx, descriptor, &insert_values, conflict_target).await?;

    // Event + audit fan-out, driven off whether the SELECT-FOR-UPDATE
    // saw a row. We don't lean on `xmax = 0`: keeping the
    // discriminator in the runtime (not the SQL) makes the rusqlite
    // mirror trivial.
    let event_kind = if inserted {
        ModelEventKind::Created
    } else {
        ModelEventKind::Updated
    };
    let audit_op = if inserted {
        AuditOperation::Create
    } else {
        AuditOperation::Update
    };
    let emits_event = if inserted {
        emits_created
    } else {
        emits_updated
    };

    if emits_event {
        enqueue_event_outbox(&mut **tx, descriptor.schema_name, event_kind, &record).await?;
    }
    let mut audit_event = None;
    if audit_enabled {
        let after = serde_json::to_value(&record).ok();
        let event = build_audit_event(descriptor, audit_op, before_snapshot, after, ctx);
        enqueue_audit_event(&mut **tx, &event).await?;
        audit_event = Some(event);
    }

    Ok((record, emits_event, audit_event))
}
