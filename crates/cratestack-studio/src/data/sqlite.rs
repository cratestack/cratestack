//! SQLite-backed [`DataSource`] for Studio.
//!
//! Studio uses `rusqlite` rather than `sqlx-sqlite` because the wider
//! workspace pins `rusqlite 0.39 → libsqlite3-sys 0.37` (via
//! `cratestack-rusqlite` and `cratestack-client-store-sqlite`), and
//! Cargo's `links = "sqlite3"` rule forbids a second
//! `libsqlite3-sys` version in the graph.
//!
//! This file is intentionally thin: it owns the [`SqliteSource`]
//! struct and the [`DataSource`] impl that delegates each method to a
//! sibling submodule. Splitting it that way keeps the dialect SQL,
//! payload bindings, async↔blocking bridge, and SQL-preview rendering
//! independently testable.

mod bindings;
mod ops;
mod preview;
mod runtime;
mod sql;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use async_trait::async_trait;
use cratestack_core::Schema;
use rusqlite::Connection;
use tokio::sync::Mutex;

use super::model_info::{PkCast, resolve_model};
use super::{ColumnSnapshot, DataError, DataSource, Page, PageRequest, Row, SqlOp, SqlPreview};

#[derive(Debug)]
pub struct SqliteSource {
    /// `rusqlite::Connection` isn't `Send` on its own but is when
    /// wrapped behind a mutex and accessed only from spawn_blocking.
    /// One connection per source — fine for Studio's expected load
    /// (a single developer browsing).
    connection: Arc<Mutex<Connection>>,
    schema: Arc<Schema>,
}

impl SqliteSource {
    pub fn new(connection: Connection, schema: Arc<Schema>) -> Self {
        Self {
            connection: Arc::new(Mutex::new(connection)),
            schema,
        }
    }
}

#[async_trait]
impl DataSource for SqliteSource {
    async fn list(&self, model: &str, page: PageRequest<'_>) -> Result<Page, DataError> {
        ops::list(&self.schema, &self.connection, model, page).await
    }

    async fn get(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::get(&self.schema, &self.connection, model, pk).await
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
            &self.connection,
            target_model,
            filter_column,
            filter_cast,
            filter_value,
            page,
        )
        .await
    }

    async fn create(&self, model: &str, payload: &Row) -> Result<Row, DataError> {
        ops::create(&self.schema, &self.connection, model, payload).await
    }

    async fn update(
        &self,
        model: &str,
        pk: &str,
        payload: &Row,
    ) -> Result<Option<Row>, DataError> {
        if payload.is_empty() {
            return self.get(model, pk).await;
        }
        ops::update(&self.schema, &self.connection, model, pk, payload).await
    }

    async fn delete(&self, model: &str, pk: &str) -> Result<Option<Row>, DataError> {
        ops::delete(&self.schema, &self.connection, model, pk).await
    }

    async fn preview_sql(
        &self,
        op: SqlOp,
        model: &str,
        pk: Option<&str>,
        payload: Option<&Row>,
    ) -> Result<SqlPreview, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        Ok(preview::render(&info, op, pk, payload))
    }

    async fn inspect_columns(
        &self,
        model: &str,
    ) -> Result<Option<Vec<ColumnSnapshot>>, DataError> {
        let (_, info) = resolve_model(&self.schema, model)?;
        let table = info.table.clone();
        let rows = runtime::with_conn(self.connection.clone(), move |conn| {
            let sql = format!("PRAGMA table_info(\"{}\")", table.replace('"', ""));
            let mut stmt = conn.prepare(&sql)?;
            let mut iter = stmt.query([])?;
            let mut out = Vec::new();
            while let Some(row) = iter.next()? {
                let name: String = row.get(1)?;
                let data_type: String = row.get(2).unwrap_or_default();
                let notnull: i64 = row.get(3).unwrap_or(0);
                out.push(ColumnSnapshot {
                    name,
                    data_type,
                    nullable: notnull == 0,
                });
            }
            Ok::<_, DataError>(out)
        })
        .await?;
        if rows.is_empty() { Ok(None) } else { Ok(Some(rows)) }
    }
}

