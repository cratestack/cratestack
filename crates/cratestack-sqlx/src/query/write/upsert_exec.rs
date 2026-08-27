//! Core upsert body: probe the conflict target under a row lock, run
//! the conflict-bearing INSERT through `upsert_resolve::resolve_upsert`
//! (which reconciles the probe's prediction against what the statement
//! actually did), then write the matching policy + audit + outbox
//! records.

use cratestack_core::{
    AuditEvent, AuditOperation, CratestackContext, CratestackError, ModelEventKind,
};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::evaluate_create_policies;
use crate::{ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, sqlx};

use super::upsert_predicate_probe::incoming_row_satisfies_predicate;
use super::upsert_prepare::prepare_upsert_insert;
use super::upsert_resolve::resolve_upsert;
use super::upsert_sql::select_for_update_by_conflict_target;

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
    // `@default(...)` column excluded from `insert_values`), and the
    // probe failure is left to propagate as an error rather than
    // guessed at.
    //
    // cratestack#745 removed the correctness argument that used to be
    // recorded here — `before_record` is no longer the only signal for
    // Created-vs-Updated, because `upsert_resolve::resolve_upsert` now
    // reconciles it against what the statement actually did, so a
    // mispredicted "no existing row" is recovered rather than
    // mislabelled. What is left is a plain preference: an unevaluable
    // predicate is a *schema* mistake, and the `Validation` error
    // `upsert_predicate_probe_error.rs` maps it to names the predicate
    // and the fix. Swallowing it would trade that diagnostic for two
    // extra round trips on every single call against such a schema.
    // Changing it is cratestack#741's territory, not this path's.
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

    // The probe's answer is a prediction, not a verdict: it locks a row
    // it finds, but it locks nothing when it finds none, so a
    // concurrent commit can still turn the "insert" prediction into a
    // real UPDATE. `resolve_upsert` runs the statement and reports what
    // the database did, recovering the update branch (policy gate and
    // before-snapshot included) when the prediction was wrong —
    // cratestack#745. See its module doc for why that is done with
    // `ON CONFLICT DO NOTHING` rather than `RETURNING (xmax = 0)`.
    let resolved = resolve_upsert(
        tx,
        runtime,
        descriptor,
        &insert_values,
        conflict_target,
        &conflict_columns,
        ctx,
        before_record,
    )
    .await?;
    let inserted = resolved.inserted;
    let record = resolved.record;

    let before_snapshot = if audit_enabled {
        resolved
            .before
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok())
    } else {
        None
    };

    // Event + audit fan-out, driven off what the statement actually
    // did. The discriminator stays in the runtime rather than the SQL
    // (no `xmax = 0`), which keeps a future rusqlite mirror a
    // transliteration of this sequencing.
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
