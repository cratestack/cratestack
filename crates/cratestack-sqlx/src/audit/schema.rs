//! Audit-log table DDL + idempotent bootstrap.

use cratestack_core::CoolError;

use crate::sqlx;

/// DDL for the audit log table. Banks typically run migrations
/// through their own tooling — this DDL is exposed so the
/// [`crate::SqlxRuntime`] can idempotently ensure the table exists
/// during bootstrap.
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

pub(crate) async fn ensure_audit_table(pool: &sqlx::PgPool) -> Result<(), CoolError> {
    // sqlx prepared statements accept only one statement per query;
    // multi-statement DDL is split on `;`. Sub-statements are
    // idempotent (`CREATE ... IF NOT EXISTS`), so this stays safe
    // under concurrent first-runs.
    for statement in AUDIT_TABLE_DDL
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        sqlx::query(statement)
            .execute(pool)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
    }
    Ok(())
}
