//! Postgres-backed audit log.
//!
//! Audit rows are written inside the same transaction as the mutation they
//! describe, guaranteeing that you can never see a committed row whose audit
//! entry didn't also commit. Downstream fan-out (Kafka, Redis pubsub) goes
//! through [`cratestack_core::AuditSink`]; this module handles the canonical
//! database record.

use cratestack_core::{AuditActor, AuditEvent, AuditOperation, CoolContext, CoolError};

use crate::ModelDescriptor;

/// DDL for the audit log table. Banks typically run migrations through their
/// own tooling — this DDL is exposed so the [`crate::SqlxRuntime`] can
/// idempotently ensure the table exists during bootstrap.
pub const AUDIT_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_audit (
    event_id UUID PRIMARY KEY,
    schema_name TEXT NOT NULL,
    model TEXT NOT NULL,
    operation TEXT NOT NULL,
    primary_key JSONB NOT NULL,
    actor JSONB NOT NULL,
    tenant TEXT,
    before JSONB,
    after JSONB,
    request_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    delivered_at TIMESTAMPTZ,
    attempts BIGINT NOT NULL DEFAULT 0,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS cratestack_audit_model_idx
    ON cratestack_audit (schema_name, model, occurred_at DESC);

CREATE INDEX IF NOT EXISTS cratestack_audit_tenant_idx
    ON cratestack_audit (tenant, occurred_at DESC)
    WHERE tenant IS NOT NULL;

CREATE INDEX IF NOT EXISTS cratestack_audit_undelivered_idx
    ON cratestack_audit (occurred_at)
    WHERE delivered_at IS NULL;
"#;

pub(crate) async fn ensure_audit_table<'e, E>(executor: E) -> Result<(), CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query(AUDIT_TABLE_DDL)
        .execute(executor)
        .await
        .map(|_| ())
        .map_err(|error| CoolError::Database(error.to_string()))
}

/// Persist an audit event into the `cratestack_audit` table. Designed to run
/// inside the same transaction as the mutation it describes.
pub(crate) async fn enqueue_audit_event<'e, E>(
    executor: E,
    event: &AuditEvent,
) -> Result<(), CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    let actor = serde_json::to_value(&event.actor)
        .map_err(|error| CoolError::Codec(format!("encode audit actor: {error}")))?;
    sqlx::query(
        "INSERT INTO cratestack_audit (\
            event_id, schema_name, model, operation, primary_key, actor, \
            tenant, before, after, request_id, occurred_at\
         ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(event.event_id)
    .bind(&event.schema_name)
    .bind(&event.model)
    .bind(event.operation.as_str())
    .bind(&event.primary_key)
    .bind(actor)
    .bind(event.tenant.as_deref())
    .bind(event.before.as_ref())
    .bind(event.after.as_ref())
    .bind(event.request_id.as_deref())
    .bind(event.occurred_at)
    .execute(executor)
    .await
    .map(|_| ())
    .map_err(|error| CoolError::Database(error.to_string()))
}

/// Derive an [`AuditActor`] from the [`CoolContext`] active at mutation time.
/// Banks generally want the principal's id, primary claims, and source IP if
/// the transport recorded one.
pub(crate) fn actor_from_context(ctx: &CoolContext) -> AuditActor {
    AuditActor {
        id: ctx.principal_actor_id().map(|s| s.to_owned()),
        claims: ctx.audit_claims_snapshot(),
        ip: ctx.client_ip().map(|s| s.to_owned()),
    }
}

/// Build an `AuditEvent` for a mutation just performed inside a transaction.
/// The caller passes the JSON snapshot(s) it has on hand — `before` for
/// updates and deletes, `after` for creates and updates — so this helper
/// stays decoupled from the SQL-row decoding path.
pub(crate) fn build_audit_event<M, PK>(
    descriptor: &'static ModelDescriptor<M, PK>,
    operation: AuditOperation,
    before: Option<serde_json::Value>,
    after: Option<serde_json::Value>,
    ctx: &CoolContext,
) -> AuditEvent {
    let primary_key = after
        .as_ref()
        .or(before.as_ref())
        .map(|snapshot| primary_key_from_snapshot(snapshot, descriptor.primary_key))
        .unwrap_or(serde_json::Value::Null);
    let before = before.map(|mut snapshot| {
        redact_snapshot(
            &mut snapshot,
            descriptor.pii_columns,
            descriptor.sensitive_columns,
        );
        snapshot
    });
    let after = after.map(|mut snapshot| {
        redact_snapshot(
            &mut snapshot,
            descriptor.pii_columns,
            descriptor.sensitive_columns,
        );
        snapshot
    });
    AuditEvent {
        event_id: uuid::Uuid::new_v4(),
        // `ModelDescriptor.schema_name` historically holds the model's Rust
        // ident; we surface it as `model`. A separate `schema_name` will be
        // wired in when the parser exposes a schema-wide label.
        schema_name: String::new(),
        model: descriptor.schema_name.to_owned(),
        operation,
        primary_key,
        actor: actor_from_context(ctx),
        tenant: ctx.tenant_id().map(|s| s.to_owned()),
        before,
        after,
        request_id: ctx.request_id().map(|s| s.to_owned()),
        occurred_at: chrono::Utc::now(),
    }
}

/// Replace values of PII/sensitive columns in a JSON snapshot with a fixed
/// marker. Banks need the audit log to record THAT a field changed without
/// retaining the actual value (PAN, SSN, address); the marker lets a human
/// reviewer see the column shifted while keeping the data out of long-term
/// logs.
pub fn redact_snapshot(
    snapshot: &mut serde_json::Value,
    pii_columns: &[&str],
    sensitive_columns: &[&str],
) {
    let Some(map) = snapshot.as_object_mut() else {
        return;
    };
    for col in pii_columns {
        if let Some(slot) = map.get_mut(*col) {
            *slot = serde_json::Value::String("[redacted-pii]".to_owned());
        }
        let camel = snake_to_camel(col);
        if camel != *col {
            if let Some(slot) = map.get_mut(&camel) {
                *slot = serde_json::Value::String("[redacted-pii]".to_owned());
            }
        }
    }
    for col in sensitive_columns {
        if let Some(slot) = map.get_mut(*col) {
            *slot = serde_json::Value::String("[redacted-sensitive]".to_owned());
        }
        let camel = snake_to_camel(col);
        if camel != *col {
            if let Some(slot) = map.get_mut(&camel) {
                *slot = serde_json::Value::String("[redacted-sensitive]".to_owned());
            }
        }
    }
}

/// Fetch the current state of a row for audit purposes, taking a row-level
/// lock so concurrent mutations cannot race us. Bypasses read policies —
/// audit reflects the actual database state, not the caller's filtered view.
pub(crate) async fn fetch_for_audit<'e, E, M, PK>(
    executor: E,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
) -> Result<Option<M>, CoolError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
    query.push(descriptor.select_projection());
    query.push(" FROM ").push(descriptor.table_name);
    query
        .push(" WHERE ")
        .push(descriptor.primary_key)
        .push(" = ");
    query.push_bind(id);
    query.push(" FOR UPDATE");
    query
        .build_query_as::<M>()
        .fetch_optional(executor)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))
}

/// Convert a model into the JSON snapshot used by the audit log. Returns
/// `None` if the model isn't serializable; that should never happen for
/// generated models which derive `Serialize`, but we degrade gracefully
/// rather than panic.
pub fn snapshot_model<T>(model: &T) -> Option<serde_json::Value>
where
    T: serde::Serialize,
{
    serde_json::to_value(model).ok()
}

/// Extract the primary-key field from a serialized model snapshot. Used to
/// stamp audit events with a stable identifier even when the schema doesn't
/// surface the PK column verbatim in the response (e.g. policy-stripped).
pub fn primary_key_from_snapshot(
    snapshot: &serde_json::Value,
    primary_key_column: &str,
) -> serde_json::Value {
    if let Some(map) = snapshot.as_object() {
        if let Some(value) = map.get(primary_key_column) {
            return value.clone();
        }
        // Try snake/camel transposition — the SQL column name might differ
        // from the JSON key emitted by the serializer.
        let camel = snake_to_camel(primary_key_column);
        if let Some(value) = map.get(&camel) {
            return value.clone();
        }
    }
    serde_json::Value::Null
}

fn snake_to_camel(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper = false;
    for ch in input.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_primary_key_by_snake_case_column() {
        let snapshot = json!({ "user_id": 42, "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, json!(42));
    }

    #[test]
    fn extracts_primary_key_via_camel_case_fallback() {
        let snapshot = json!({ "userId": 42, "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, json!(42));
    }

    #[test]
    fn returns_null_when_primary_key_absent() {
        let snapshot = json!({ "balance": "10.00" });
        let pk = primary_key_from_snapshot(&snapshot, "user_id");
        assert_eq!(pk, serde_json::Value::Null);
    }

    #[test]
    fn snapshot_round_trip_preserves_strings_and_numbers() {
        let snap =
            snapshot_model(&json!({ "amount": "12.34", "currency": "USD" })).expect("serializable");
        assert_eq!(snap["amount"], json!("12.34"));
        assert_eq!(snap["currency"], json!("USD"));
    }

    #[test]
    fn audit_operation_string_is_lowercase() {
        assert_eq!(AuditOperation::Create.as_str(), "create");
        assert_eq!(AuditOperation::Update.as_str(), "update");
        assert_eq!(AuditOperation::Delete.as_str(), "delete");
    }

    #[test]
    fn redacts_pii_columns_with_canned_marker() {
        let mut snap = json!({
            "id": 1,
            "email": "alice@example.com",
            "balance": "10.00",
        });
        redact_snapshot(&mut snap, &["email"], &[]);
        assert_eq!(snap["email"], json!("[redacted-pii]"));
        assert_eq!(snap["balance"], json!("10.00"));
    }

    #[test]
    fn redacts_sensitive_columns_with_distinct_marker() {
        let mut snap = json!({
            "id": 1,
            "risk_score": 87,
        });
        redact_snapshot(&mut snap, &[], &["risk_score"]);
        assert_eq!(snap["risk_score"], json!("[redacted-sensitive]"));
    }

    #[test]
    fn redaction_handles_camel_case_keys() {
        let mut snap = json!({
            "id": 1,
            "primaryEmail": "x@y.com",
        });
        redact_snapshot(&mut snap, &["primary_email"], &[]);
        assert_eq!(snap["primaryEmail"], json!("[redacted-pii]"));
    }

    #[test]
    fn redaction_is_noop_for_absent_columns() {
        let mut snap = json!({ "id": 1 });
        redact_snapshot(&mut snap, &["email"], &["risk_score"]);
        assert_eq!(snap, json!({ "id": 1 }));
    }
}
