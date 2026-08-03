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
//! `CREATE TABLE` instead (`emit::postgres::tables`). Expression and
//! partial indexes (`indexprs`/`indpred` not null) are excluded too:
//! `AddIndex` has no way to represent either, so guessing a plain
//! column list for one would misrepresent it — skipped rather than
//! guessed, same rule as unmapped columns.

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
                array_agg(a.attname ORDER BY k.ord) AS columns \
         FROM pg_index i \
         JOIN pg_class ic ON ic.oid = i.indexrelid \
         JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, ord) ON true \
         JOIN pg_attribute a ON a.attrelid = i.indrelid AND a.attnum = k.attnum \
         WHERE i.indrelid = to_regclass($1) \
           AND i.indisprimary = false \
           AND i.indexprs IS NULL \
           AND i.indpred IS NULL \
         GROUP BY ic.relname, i.indisunique \
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
        out.push(AddIndex {
            name,
            table: table.to_owned(),
            columns,
            unique,
        });
    }
    Ok(out)
}
