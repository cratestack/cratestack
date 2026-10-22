//! Client-side bookkeeping tables and the per-model undo log.
//!
//! The undo log is what makes rebasing correct for *any* local mutator, not
//! just idempotent ones. While a local mutator runs, triggers copy each row
//! it touches into a shadow table (first touch only) and mark the key dirty.
//! Rewinding is then mechanical: delete every dirty key, re-insert the
//! shadows, and the table is back to exactly the last server state — with
//! storage-class fidelity, since the shadow holds the row as stored, not a
//! re-encoding of it. A rejected mutation's local effect disappears the
//! same way.

pub(crate) const CLIENT_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_sync_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cratestack_sync_pending (
    seq INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    args TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cratestack_sync_rejected (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    seq INTEGER NOT NULL,
    name TEXT NOT NULL,
    args TEXT NOT NULL,
    created_at TEXT NOT NULL,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    rejected_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS cratestack_sync_dirty (
    model TEXT NOT NULL,
    pk NOT NULL,
    PRIMARY KEY (model, pk)
);
CREATE TABLE IF NOT EXISTS cratestack_sync_capture (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    active INTEGER NOT NULL
);
INSERT OR IGNORE INTO cratestack_sync_capture (id, active) VALUES (1, 0);
"#;

const CAPTURING: &str = "(SELECT active FROM cratestack_sync_capture WHERE id = 1) = 1";

pub(crate) fn shadow_table(table: &str) -> String {
    format!("cratestack_sync_shadow_{table}")
}

/// Shadow table and capture triggers for one synced model. The shadow is
/// rebuilt whenever it is empty, so it follows the base table's columns
/// across app upgrades; a non-empty shadow is only ever rewound, never
/// rebuilt.
pub(crate) fn model_ddl(model: &str, table: &str, pk: &str) -> String {
    let shadow = shadow_table(table);
    // `WHERE NOT EXISTS`, not `INSERT OR IGNORE`: inside a trigger the outer
    // statement's conflict policy overrides the trigger's own, so an upsert
    // (`ON CONFLICT DO UPDATE`) firing these would abort on the duplicate.
    let not_dirty = |row: &str| {
        format!(
            "NOT EXISTS (SELECT 1 FROM cratestack_sync_dirty WHERE model = '{model}' AND pk = {row}.{pk})"
        )
    };
    let capture_old = format!(
        "INSERT INTO {shadow} SELECT * FROM {table} WHERE {pk} = OLD.{pk} AND {}; \
         INSERT INTO cratestack_sync_dirty (model, pk) SELECT '{model}', OLD.{pk} WHERE {};",
        not_dirty("OLD"),
        not_dirty("OLD"),
    );
    let mark_new = format!(
        "INSERT INTO cratestack_sync_dirty (model, pk) SELECT '{model}', NEW.{pk} WHERE {};",
        not_dirty("NEW"),
    );
    format!(
        r#"
CREATE TABLE IF NOT EXISTS {shadow} AS SELECT * FROM {table} WHERE 0;
CREATE TRIGGER IF NOT EXISTS {shadow}_bu BEFORE UPDATE ON {table} WHEN {CAPTURING}
BEGIN {capture_old} END;
CREATE TRIGGER IF NOT EXISTS {shadow}_au AFTER UPDATE ON {table} WHEN {CAPTURING}
BEGIN {mark_new} END;
CREATE TRIGGER IF NOT EXISTS {shadow}_bd BEFORE DELETE ON {table} WHEN {CAPTURING}
BEGIN {capture_old} END;
CREATE TRIGGER IF NOT EXISTS {shadow}_ai AFTER INSERT ON {table} WHEN {CAPTURING}
BEGIN {mark_new} END;
"#
    )
}

/// Restore every dirty row of one model to its pre-mutation state.
pub(crate) fn rewind_sql(model: &str, table: &str, pk: &str) -> String {
    let shadow = shadow_table(table);
    format!(
        "DELETE FROM {table} WHERE {pk} IN \
           (SELECT pk FROM cratestack_sync_dirty WHERE model = '{model}'); \
         INSERT INTO {table} SELECT * FROM {shadow}; \
         DELETE FROM {shadow};"
    )
}

pub(crate) fn set_capture(active: bool) -> &'static str {
    if active {
        "UPDATE cratestack_sync_capture SET active = 1 WHERE id = 1"
    } else {
        "UPDATE cratestack_sync_capture SET active = 0 WHERE id = 1"
    }
}
