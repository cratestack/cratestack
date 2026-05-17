//! Postgres-backed [`DataSource`] for Studio.
//!
//! The dynamic-row-to-JSON path uses Postgres's `row_to_json(t)` so the
//! fetch path doesn't have to decode per-column type OIDs in Rust.
//! Each query projects the model's columns into a subquery, then
//! wraps the whole thing in `row_to_json`.
//!
//! This entry file owns the [`PostgresSource`] struct and the
//! [`DataSource`] impl; SQL builders, payload bindings, and CRUD ops
//! live in sibling submodules so they stay independently testable.

mod bindings;
mod ops;
mod preview;
mod sql;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use sqlx_core::row::Row as _;
use sqlx_postgres::{PgPool, PgRow};

use super::model_info::{PkCast, resolve_model};
use super::{ColumnSnapshot, DataError, DataSource, Page, PageRequest, Row, SqlOp, SqlPreview};

#[derive(Debug, Clone)]
pub struct PostgresSource {
    pool: PgPool,
    schema: Arc<Schema>,
}

impl PostgresSource {
    pub fn new(pool: PgPool, schema: Arc<Schema>) -> Self {
        Self { pool, schema }
    }
}

#[async_trait]
impl DataSource for PostgresSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        ops::list(&self.schema, &self.pool, model, page).await
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::get(&self.schema, &self.pool, model, pk).await
    }

    async fn follow(
        &self,
        target_model: &str,
        filter_column: &str,
        filter_cast: PkCast,
        filter_value: &str,
        page: PageRequest<'_>,
    ) -> Result<Page, DataError> {
        ops::follow(
            &self.schema,
            &self.pool,
            target_model,
            filter_column,
            filter_cast,
            filter_value,
            page,
        )
        .await
    }

    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError> {
        ops::create(&self.schema, &self.pool, model, payload).await
    }

    async fn update(&self, model: &str, pk: &str, payload: &Row) -> Result<Option<Row>, DataError> {
        if payload.is_empty() {
            return self.get(model, pk).await;
        }
        ops::update(&self.schema, &self.pool, model, pk, payload).await
    }

    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::delete(&self.schema, &self.pool, model, pk).await
    }

    async fn preview_sql(
        &self,
        op: SqlOp,
        model: &str,
        pk: Option<&str>,
        payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        Ok(preview::render(&self.schema, &info, model, op, pk, payload))
    }

    async fn inspect_columns(&self, model: &str) -> Result<Option<Vec<ColumnSnapshot>>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let sql = "SELECT column_name, data_type, is_nullable \
                   FROM information_schema.columns \
                   WHERE table_schema = current_schema() \
                     AND table_name = $1 \
                   ORDER BY ordinal_position";
        let rows: Vec<PgRow> = sqlx_core::query::query(sql)
            .bind(info.table.clone())
            .fetch_all(&self.pool)
            .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            rows.into_iter()
                .map(|r| {
                    let name: String = r.try_get(0).unwrap_or_default();
                    let data_type: String = r.try_get(1).unwrap_or_default();
                    let is_nullable: String = r.try_get(2).unwrap_or_default();
                    ColumnSnapshot {
                        name,
                        data_type,
                        nullable: is_nullable.eq_ignore_ascii_case("YES"),
                    }
                })
                .collect(),
        ))
    }
}
