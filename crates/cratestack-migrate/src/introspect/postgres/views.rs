//! View introspection: `pg_views`/`pg_matviews` for the SQL body,
//! `pg_depend` for the source tables a view's body reads from.
//!
//! `ViewProjection::primary_key` has no catalog equivalent — a view's
//! `@id` field is a `.cstack`-source-level annotation with zero trace
//! in `pg_catalog`, so it's always the empty string here, matching a
//! schema-side view declared with no `@id` field
//! (`crate::diff::views::project_view`'s `unwrap_or_default()`). A
//! `.cstack` view that *does* declare `@id` will therefore always show
//! up as drift on this one field when diffed against an introspected
//! view — a known, documented gap for Phase C to surface clearly
//! rather than a bug here.

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use crate::ViewProjection;

use super::error::IntrospectError;

pub(super) async fn introspect_views(
    pool: &PgPool,
) -> Result<Vec<ViewProjection>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT c.relname, pg_get_viewdef(c.oid, true), (c.relkind = 'm') AS is_materialized \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind IN ('v', 'm') \
           AND n.nspname = current_schema() \
         ORDER BY c.relname",
    )
    .fetch_all(pool)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let name: String = row.try_get(0)?;
        let sql: String = row.try_get(1)?;
        let is_materialized: bool = row.try_get(2)?;
        let source_tables = introspect_source_tables(pool, &name).await?;
        out.push(ViewProjection {
            name,
            sql,
            is_materialized,
            primary_key: String::new(),
            source_tables,
        });
    }
    Ok(out)
}

/// Ordinary tables (`pg_depend`'s referenced relation must itself be
/// `relkind = 'r'`) the view's rewrite rule reads from, deduplicated
/// and in name order. `deptype = 'n'` ("normal") is the dependency
/// type Postgres records for a view depending on the tables its
/// `SELECT` references.
async fn introspect_source_tables(
    pool: &PgPool,
    view: &str,
) -> Result<Vec<String>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT DISTINCT dep.relname \
         FROM pg_depend d \
         JOIN pg_rewrite r ON r.oid = d.objid \
         JOIN pg_class dep ON dep.oid = d.refobjid \
         WHERE r.ev_class = to_regclass($1) \
           AND d.deptype = 'n' \
           AND dep.relkind = 'r' \
         ORDER BY dep.relname",
    )
    .bind(view)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| row.try_get::<String, _>(0).map_err(IntrospectError::from))
        .collect()
}
