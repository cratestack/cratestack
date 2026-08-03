//! Table list — `pg_class` filtered to ordinary tables in the current
//! schema, excluding `cratestack_migrations` itself (design doc §5.2).

use sqlx_core::row::Row as _;
use sqlx_postgres::PgPool;

use super::error::IntrospectError;

/// Every ordinary (`relkind = 'r'`) table in `current_schema()`, in
/// name order, minus `cratestack_migrations`. Views (`relkind = 'v'`)
/// and materialized views (`'m'`) are excluded here — [`super::views`]
/// introspects those separately.
pub(super) async fn list_tables(pool: &PgPool) -> Result<Vec<String>, IntrospectError> {
    let rows = sqlx_core::query::query(
        "SELECT c.relname \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind = 'r' \
           AND n.nspname = current_schema() \
           AND c.relname <> 'cratestack_migrations' \
         ORDER BY c.relname",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| row.try_get::<String, _>(0))
        .collect::<Result<Vec<_>, _>>()?)
}
