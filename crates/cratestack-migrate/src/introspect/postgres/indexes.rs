//! Index introspection: `pg_index` + `pg_class` + `pg_attribute`.
//!
//! Matches against `naming.rs`'s existing index-naming convention "for
//! free" rather than by recomputing it: a cratestack-generated
//! migration always creates a `@unique`/`@@unique` index under the
//! exact name `crate::naming::index_name_unique` would compute (see
//! `emit::postgres::indexes`), so reading the index's *actual* name
//! back out of `pg_class` already agrees with what
//! `crate::convert::project_model` would produce for the equivalent
//! `.cstack` field — no separate reverse-engineering needed. The diff
//! engine matches indexes by name (`crate::diff::indexes`), so this is
//! exactly what's required for a baselined-then-later-diffed index to
//! not look renamed.
//!
//! The primary key's own implicit unique index (`indisprimary = true`)
//! is excluded — the schema-side projection never emits a standalone
//! `AddIndex` for a primary key; `PRIMARY KEY (...)` is inline on
//! `CREATE TABLE` instead (`emit::postgres::tables`). Expression indexes
//! (`indexprs` not null) are excluded too: `AddIndex` has no way to
//! represent one, so guessing a plain column list for it would
//! misrepresent it — skipped rather than guessed, same rule as unmapped
//! columns.
//!
//! Partial indexes (`indpred` not null, cratestack#742) are read via
//! `pg_get_expr(i.indpred, i.indrelid)`, the same catalog function
//! Postgres's own `\d` / `pg_dump` use to deparse a stored predicate
//! back into SQL text. That text is **normalized** — parenthesized,
//! literals given explicit type casts, identifiers case-folded — so it
//! is not, in general, byte-identical to the `.cstack` schema's literal
//! `where: "..."` text even when nothing changed. `crate::diff::indexes`
//! tolerates the normalization empirically observed against a live
//! Postgres 18 (whitespace collapsing, exactly one wrapping pair of
//! parens, and an explicit `::type` cast inserted onto every literal —
//! see `crate::diff::indexes::predicate`'s module doc) when deciding
//! whether a partial index's predicate changed; it does not attempt to
//! replicate identifier case-folding, which would need a real SQL
//! expression parser (out of scope, see cratestack#742's "Out of
//! Scope").
//!
//! **Adoption note (blast-radius change, cratestack#742):** before this
//! ticket, the query below carried an `AND i.indpred IS NULL` clause,
//! which excluded every partial index from introspection outright — the
//! same "skip rather than guess" treatment expression indexes still get
//! above, since `AddIndex` had no `where_predicate` field to represent
//! one in yet. A side effect of that exclusion was that a partial index
//! created outside Cratestack (unmanaged, undeclared in any `.cstack`
//! schema — cratestack#742's own motivating scenario, see the ticket's
//! Intent section) was invisible to `migrate` and therefore could never
//! be touched by it. Now that `AddIndex` can represent a partial index,
//! that exclusion is gone and every partial index enters the diff like
//! any other index: one absent from the schema is a bare `DROP INDEX`
//! candidate (`crate::diff::indexes`/`emit::postgres::indexes`, no
//! `CASCADE`) on the very next `migrate` run — deliberately, for
//! consistency with how an ordinary (non-partial) unmanaged index is
//! already treated, not a special case. **If you're upgrading past this
//! change and have a hand-made partial index that no `.cstack` schema
//! declares, the next `migrate` run will drop it** — declare it via
//! `@@unique`/`@@index([...], where: "...")` first if you want to keep
//! it. Pinned by
//! `crates/cratestack-migrate/tests/postgres_introspect.rs`'s
//! `undeclared_partial_index_is_dropped_by_diff`.

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use crate::ir::AddIndex;

use super::error::IntrospectError;

pub(super) async fn introspect_indexes(
    pool: &PgPool,
    table: &str,
) -> Result<Vec<AddIndex>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT ic.relname, i.indisunique, \
                array_agg(a.attname ORDER BY k.ord) AS columns, \
                pg_get_expr(i.indpred, i.indrelid) AS predicate \
         FROM pg_index i \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
         WHERE i.indrelid = to_regclass($1) \
           AND i.indisprimary = false \
           AND i.indexprs IS NULL \
         GROUP BY ic.relname, i.indisunique, i.indpred, i.indrelid \
         ORDER BY ic.relname",
    )
    .bind(table)
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get(0)?;
        let unique: bool = row.try_get(1)?;
        let columns: Vec<String> = row.try_get(2)?;
        let where_predicate: Option<String> = row.try_get(3)?;
        out.push(AddIndex {
            name,
            table: table.to_owned(),
            columns,
            unique,
            // Live introspection doesn't (yet) read `pg_am`/`pg_opclass`
            // back out, so a re-introspected `ivfflat`/`hnsw` index
            // always round-trips as `using: None, opclass: None` here.
            // Harmless for the diff engine — indexes are matched by
            // name only (`crate::diff::indexes`), never by these
            // fields — but it does mean introspection can't yet tell an
            // ANN index apart from a plain one. Tracked as a follow-up,
            // not a regression: no code path produced these fields
            // before issue #156 either.
            using: None,
            opclass: None,
            where_predicate,
        });
    }
    Ok(out)
}
