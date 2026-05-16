//! Postgres-backed audit log. Audit rows write inside the mutation's
//! transaction — you can never see a committed row whose audit entry
//! didn't also commit. Fan-out (Kafka/Redis pubsub) goes through
//! [`cratestack_core::AuditSink`]; this module is the canonical DB
//! record.

mod redact;
mod schema;

use cratestack_core::{AuditActor, AuditEvent, AuditOperation, CoolContext, CoolError};

use crate::ModelDescriptor;
use crate::sqlx;

pub use redact::{primary_key_from_snapshot, redact_snapshot, snapshot_model};
pub use schema::AUDIT_TABLE_DDL;
pub(crate) use schema::ensure_audit_table;

/// Persist an audit event into the `cratestack_audit` table. Designed
/// to run inside the same transaction as the mutation it describes.
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

/// Derive an [`AuditActor`] from the [`CoolContext`] active at
/// mutation time. Banks generally want the principal's id, primary
/// claims, and source IP if the transport recorded one.
pub(crate) fn actor_from_context(ctx: &CoolContext) -> AuditActor {
    AuditActor {
        id: ctx.principal_actor_id().map(|s| s.to_owned()),
        claims: ctx.audit_claims_snapshot(),
        ip: ctx.client_ip().map(|s| s.to_owned()),
    }
}

/// Build an `AuditEvent` for a mutation just performed in a tx. The
/// caller passes the JSON snapshot(s) on hand (`before` for
/// updates/deletes, `after` for creates/updates) so this helper stays
/// decoupled from the SQL-row decoding path.
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
        // descriptor.schema_name historically holds the Rust ident;
        // surfaced as `model`. A schema-wide label will be wired in
        // when the parser exposes one.
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

/// Fetch the current state of a row for audit purposes, taking a
/// row-level lock so concurrent mutations cannot race us. Bypasses
/// read policies — audit reflects the actual database state, not the
/// caller's filtered view.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_operation_string_is_lowercase() {
        assert_eq!(AuditOperation::Create.as_str(), "create");
        assert_eq!(AuditOperation::Update.as_str(), "update");
        assert_eq!(AuditOperation::Delete.as_str(), "delete");
    }
}
