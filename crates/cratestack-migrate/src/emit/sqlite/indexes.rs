//! Index DDL: CREATE INDEX and DROP INDEX.

use std::fmt::Write as _;

use crate::ir::{AddIndex, DropIndex};

use super::idents::quote_ident;

/// SQLite's `CREATE INDEX` has no `USING <method>` clause or per-column
/// operator classes — there is no pluggable index-access-method concept
/// to select (pgvector, and `@@index([...], using: ..., opclass: ...)`'s
/// `using`/`opclass` fields, are inherently Postgres-only; issue #156).
/// `index.using`/`index.opclass` are intentionally ignored here rather
/// than rejected: a plain index over the same columns is still valid
/// SQLite DDL, and `include_embedded_schema!` already rejects
/// `extension pgvector { }` outright (see `docs/design/extensions.md`
/// §6), so an embedded-target schema can't reach this with a pgvector
/// `using` value in practice.
pub(super) fn emit_add_index(sql: &mut String, index: &AddIndex) {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let columns: Vec<String> = index.columns.iter().map(|c| quote_ident(c)).collect();
    writeln!(
        sql,
        "CREATE {unique}INDEX {} ON {} ({});",
        quote_ident(&index.name),
        quote_ident(&index.table),
        columns.join(", ")
    )
    .unwrap();
}

pub(super) fn emit_drop_index(sql: &mut String, drop: &DropIndex) {
    writeln!(sql, "DROP INDEX {};", quote_ident(&drop.name)).unwrap();
}
