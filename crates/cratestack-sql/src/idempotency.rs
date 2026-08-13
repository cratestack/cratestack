//! Idempotency DDL and utilities for Postgres.

/// SQL DDL for the idempotency table. Banks typically run migrations through
/// their own tooling — `cratestack` currently ships migrations as raw DDL
/// since the migration engine is deferred to Phase 3.
pub const IDEMPOTENCY_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_idempotency (
    principal_fingerprint TEXT NOT NULL,
    key TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    reservation_id UUID NOT NULL,
    response_status INT,
    response_headers BYTEA,
    response_body BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (principal_fingerprint, key)
);

CREATE INDEX IF NOT EXISTS cratestack_idempotency_expires_idx
    ON cratestack_idempotency (expires_at);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idempotency_table_ddl_contains_table_creation() {
        assert!(
            IDEMPOTENCY_TABLE_DDL.contains("CREATE TABLE IF NOT EXISTS cratestack_idempotency")
        );
    }

    #[test]
    fn idempotency_table_ddl_contains_index_creation() {
        assert!(
            IDEMPOTENCY_TABLE_DDL
                .contains("CREATE INDEX IF NOT EXISTS cratestack_idempotency_expires_idx")
        );
    }

    #[test]
    fn idempotency_table_ddl_contains_primary_key() {
        assert!(IDEMPOTENCY_TABLE_DDL.contains("PRIMARY KEY (principal_fingerprint, key)"));
    }
}
