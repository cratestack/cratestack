//! Server-side tables and the per-table capture trigger.
//!
//! Change capture is a Postgres trigger rather than a hook in the generated
//! write path, so a write reaches syncing clients **however it was made**:
//! a sync push, a generated REST/RPC route, a procedure, a migration, or
//! `psql`. That is what makes server-originated mutations propagate without
//! the application doing anything.
//!
//! The change log is *compacted*: one row per `(scope, model, pk)`, moved
//! to a new position on every write. A client that was offline for a month
//! receives each changed row once, not every intermediate version. The row
//! itself is not stored — pulls fetch the current row through the model's
//! read policy (see `fetch.rs`) — so the log stays small and never leaks a
//! value the puller is no longer allowed to read.

/// Tables, the position sequence, and the `cratestack_sync_record` function
/// the capture triggers call. Idempotent; run it at boot.
pub const SERVER_DDL: &str = r#"
CREATE SEQUENCE IF NOT EXISTS cratestack_sync_change_seq;

CREATE TABLE IF NOT EXISTS cratestack_sync_changes (
    scope TEXT NOT NULL,
    model TEXT NOT NULL,
    pk TEXT NOT NULL,
    op TEXT NOT NULL CHECK (op IN ('upsert', 'delete')),
    inline_row JSONB,
    txid XID8 NOT NULL DEFAULT pg_current_xact_id(),
    seq BIGINT NOT NULL DEFAULT nextval('cratestack_sync_change_seq'),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (scope, model, pk)
);
CREATE INDEX IF NOT EXISTS cratestack_sync_changes_position
    ON cratestack_sync_changes (txid, seq);
CREATE INDEX IF NOT EXISTS cratestack_sync_changes_tombstones
    ON cratestack_sync_changes (changed_at) WHERE op = 'delete';

CREATE TABLE IF NOT EXISTS cratestack_sync_clients (
    client_id UUID PRIMARY KEY,
    principal TEXT NOT NULL,
    label TEXT,
    last_mutation_seq BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS cratestack_sync_clients_principal
    ON cratestack_sync_clients (principal);
CREATE INDEX IF NOT EXISTS cratestack_sync_clients_last_seen
    ON cratestack_sync_clients (last_seen_at);

CREATE TABLE IF NOT EXISTS cratestack_sync_horizon (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (id),
    txid XID8 NOT NULL,
    seq BIGINT NOT NULL
);
INSERT INTO cratestack_sync_horizon (id, txid, seq)
    VALUES (TRUE, '0'::xid8, 0) ON CONFLICT DO NOTHING;

CREATE OR REPLACE FUNCTION cratestack_sync_record(
    p_scope TEXT, p_model TEXT, p_pk TEXT, p_op TEXT, p_inline JSONB
) RETURNS VOID LANGUAGE plpgsql AS $fn$
BEGIN
    IF p_scope IS NULL THEN
        RETURN;
    END IF;
    INSERT INTO cratestack_sync_changes (scope, model, pk, op, inline_row)
    VALUES (p_scope, p_model, p_pk, p_op, p_inline)
    ON CONFLICT (scope, model, pk) DO UPDATE SET
        op = EXCLUDED.op,
        inline_row = EXCLUDED.inline_row,
        txid = EXCLUDED.txid,
        seq = EXCLUDED.seq,
        changed_at = EXCLUDED.changed_at;
    -- Delivered at COMMIT, never for a rolled-back write, and de-duplicated
    -- per transaction by Postgres — a bulk update pokes each scope once.
    PERFORM pg_notify('cratestack_sync', p_scope);
END
$fn$;
"#;

/// Which table to capture, and how to compute a row's scope.
#[derive(Debug, Clone, Copy)]
pub struct CaptureSpec<'a> {
    /// Physical table, e.g. a generated descriptor's `table_name`.
    pub table: &'a str,
    /// Model name clients see in [`crate::protocol::Change::model`].
    pub model: &'a str,
    /// Primary-key column, e.g. a descriptor's `primary_key`.
    pub pk_column: &'a str,
    /// A SQL expression over the row variable `r` yielding the row's scope
    /// as `TEXT`, or `NULL` for "not synced". Trusted developer input, e.g.
    /// `'user:' || r."ownerId"`.
    pub scope_sql: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a plain SQL identifier ([A-Za-z_][A-Za-z0-9_]*)")]
pub struct IdentifierError(pub String);

fn identifier(value: &str) -> Result<&str, IdentifierError> {
    let mut chars = value.chars();
    let valid_start = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    if valid_start && chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(value)
    } else {
        Err(IdentifierError(value.to_owned()))
    }
}

/// DDL installing the capture trigger on one table. Idempotent.
///
/// A row whose scope changes on `UPDATE` produces a tombstone in the old
/// scope *and* an upsert in the new one, in that order — so a client that
/// loses access sees a delete, and one that gains it sees the row.
pub fn capture_trigger_sql(spec: CaptureSpec<'_>) -> Result<String, IdentifierError> {
    let table = identifier(spec.table)?;
    let model = identifier(spec.model)?;
    let pk = identifier(spec.pk_column)?;
    let scope = spec.scope_sql;
    Ok(format!(
        r#"
CREATE OR REPLACE FUNCTION cratestack_sync_capture_{table}() RETURNS trigger
LANGUAGE plpgsql AS $fn$
DECLARE
    r RECORD;
    old_scope TEXT;
    new_scope TEXT;
BEGIN
    IF TG_OP IN ('UPDATE', 'DELETE') THEN
        r := OLD;
        old_scope := ({scope});
    END IF;
    IF TG_OP IN ('INSERT', 'UPDATE') THEN
        r := NEW;
        new_scope := ({scope});
    END IF;
    IF old_scope IS NOT NULL AND old_scope IS DISTINCT FROM new_scope THEN
        PERFORM cratestack_sync_record(old_scope, '{model}', (OLD."{pk}")::text, 'delete', NULL);
    END IF;
    IF new_scope IS NOT NULL THEN
        PERFORM cratestack_sync_record(new_scope, '{model}', (NEW."{pk}")::text, 'upsert', NULL);
    END IF;
    RETURN NULL;
END
$fn$;
DROP TRIGGER IF EXISTS cratestack_sync_capture ON "{table}";
CREATE TRIGGER cratestack_sync_capture
    AFTER INSERT OR UPDATE OR DELETE ON "{table}"
    FOR EACH ROW EXECUTE FUNCTION cratestack_sync_capture_{table}();
"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_identifiers_that_would_need_quoting() {
        let spec = CaptureSpec {
            table: "notes\"; DROP TABLE x; --",
            model: "Note",
            pk_column: "id",
            scope_sql: "'all'",
        };
        assert!(capture_trigger_sql(spec).is_err());
    }

    #[test]
    fn renders_a_trigger_for_a_plain_table() {
        let sql = capture_trigger_sql(CaptureSpec {
            table: "notes",
            model: "Note",
            pk_column: "id",
            scope_sql: "'user:' || r.\"ownerId\"",
        })
        .unwrap();
        assert!(sql.contains("cratestack_sync_capture_notes()"));
        assert!(sql.contains("AFTER INSERT OR UPDATE OR DELETE ON \"notes\""));
    }
}
