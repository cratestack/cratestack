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
///
/// `index.where_predicate` (issue #742), by contrast, **is** rendered:
/// SQLite has supported partial indexes (`CREATE INDEX ... WHERE
/// <predicate>`) since 3.8.0, the same syntax Postgres uses, so this is
/// covered rather than diverging. The one real backend difference is in
/// what a predicate may legally reference, not in the syntax: SQLite
/// requires a partial index's `WHERE` clause to be a deterministic
/// expression over columns of the *indexed table only* (no
/// subqueries, no non-deterministic functions, no references to other
/// tables) — https://www.sqlite.org/partialindex.html. `.cstack`
/// predicates are still carried through verbatim, unvalidated (same
/// posture as Postgres), so a predicate that violates SQLite's
/// restriction surfaces as a SQLite error at migration-apply time, not
/// at schema-check time.
pub(super) fn emit_add_index(sql: &mut String, index: &AddIndex) {
    let unique = if index.unique { "UNIQUE " } else { "" };
    let columns: Vec<String> = index.columns.iter().map(|c| quote_ident(c)).collect();
    let where_clause = match index.where_predicate.as_deref() {
        Some(predicate) => format!(" WHERE {predicate}"),
        None => String::new(),
    };
    writeln!(
        sql,
        "CREATE {unique}INDEX {} ON {} ({}){where_clause};",
        quote_ident(&index.name),
        quote_ident(&index.table),
        columns.join(", ")
    )
    .unwrap();
}

pub(super) fn emit_drop_index(sql: &mut String, drop: &DropIndex) {
    writeln!(sql, "DROP INDEX {};", quote_ident(&drop.name)).unwrap();
}
