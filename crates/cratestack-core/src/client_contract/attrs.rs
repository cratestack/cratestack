//! The reviewed drop list: attributes that never change how an op's bytes
//! decode, so they are out of the op contract. Everything not named here
//! stays in, including attributes cratestack adds later: a new attribute
//! moves digests until someone adds it below, with a test and a reason
//! (#1065's fail-loud rule, applied to a smaller set).
//!
//! Matching is by exact name (`@Deny` is not `@deny`), so an unknown or
//! misspelled attribute is kept.

/// `(attribute name, why it is wire-neutral)`. Each entry is checked by
/// `tests_drop.rs`, which adds it to a field, a model and a procedure and
/// asserts no op digest moves.
pub const DROPPED_ATTRIBUTES: &[(&str, &str)] = &[
    (
        "@@allow",
        "policy: decides who may call, not how bytes decode",
    ),
    ("@@deny", "policy"),
    ("@allow", "procedure policy"),
    ("@deny", "procedure policy"),
    ("@authorize", "procedure policy re-check"),
    ("@@index", "storage: a database index"),
    (
        "@@unique",
        "storage: upsert targets are unsupported, so no input shape depends on it",
    ),
    ("@@map", "storage: table name"),
    ("@map", "storage: column name"),
    ("@@sql", "a view's or query's SQL body"),
    ("@@server_sql", "a view's Postgres body"),
    ("@@embedded_sql", "a view's SQLite body"),
    ("@@materialized", "a view's storage"),
    ("@@no_unique", "a view's key opt-out"),
    ("@@audit", "audit logging"),
    ("@@retain", "retention of audit rows"),
    (
        "@@soft_delete",
        "a stored `deleted_at` column that is on no wire shape",
    ),
    ("@@rename", "migration marker"),
    ("@rename", "migration marker"),
    (
        "@@internal",
        "decides whether an op exists, which is the op key",
    ),
    (
        "@@subscribe",
        "decides whether `subscribe` exists, which is its key",
    ),
    (
        "@@emit",
        "the event kinds belong to the `subscribe` op's own contract; \
         `ModelEvent<T>` and its kind enum are fixed",
    ),
    ("@@mcp", "MCP exposure"),
    ("@mcp", "MCP exposure"),
    ("@no_idempotency", "retry policy, not a shape"),
    ("@no_rate_limit", "rate-limit participation"),
    ("@isolation", "transaction isolation of the handler"),
    ("@length", "validator: only rejects"),
    ("@range", "validator"),
    ("@regex", "validator"),
    ("@email", "validator"),
    ("@uri", "validator"),
    ("@iso4217", "validator"),
];

/// The exact name of an attribute's text: `@@allow("read", x)` is
/// `@@allow`, `@length(min: 1)` is `@length`. The sigil is part of the
/// name, so `@allow` and `@@allow` stay distinct.
pub fn attribute_name(raw: &str) -> &str {
    let end = raw
        .find(|c: char| !(c == '@' || c == '_' || c.is_ascii_alphanumeric()))
        .unwrap_or(raw.len());
    &raw[..end]
}

/// Whether `raw` is on the drop list.
pub fn is_dropped(raw: &str) -> bool {
    let name = attribute_name(raw);
    DROPPED_ATTRIBUTES
        .iter()
        .any(|(dropped, _)| *dropped == name)
}

/// Whether a field is `@server_only`: on no wire since 0.13.0, so out of
/// every projection.
pub(super) fn is_server_only(raw: &str) -> bool {
    raw == "@server_only"
}
