//! Bulk DELETE-by-predicate: one statement tombstones (soft-delete)
//! or removes (hard-delete) every row matching the filter AND the
//! delete policy.
//!
//! Same shape as `update_many` — refuses to run without ≥1 filter so
//! callers can't accidentally truncate a table at the typed-builder
//! level.

use cratestack_core::{BatchSummary, CoolContext, CoolError};

use crate::{FilterExpr, ModelDescriptor, SqlxRuntime, sqlx};

use super::delete_many_exec::run_delete_many_in_tx;

#[derive(Debug, Clone)]
pub struct DeleteMany<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> DeleteMany<'a, M, PK> {
    pub fn where_(mut self, filter: crate::Filter) -> Self {
        self.filters.push(FilterExpr::from(filter));
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.filters.push(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.filters.push(FilterExpr::any(filters));
        self
    }

    /// Conditionally append a filter; `None` is a no-op.
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    /// Approximate SQL preview. The runtime path interpolates filter
    /// predicates and the delete policy clause; this returns the rough
    /// shape for migration tooling and the schema studio.
    pub fn preview_sql(&self) -> String {
        let mut sql = match self.descriptor.soft_delete_column {
            Some(col) => {
                let mut s = format!("UPDATE {} SET {col} = NOW()", self.descriptor.table_name);
                if let Some(version_col) = self.descriptor.version_column {
                    s.push_str(&format!(", {version_col} = {version_col} + 1"));
                }
                s.push_str(&format!(" WHERE {col} IS NULL AND "));
                s
            }
            None => format!("DELETE FROM {} WHERE ", self.descriptor.table_name),
        };
        sql.push_str("<filters> AND <delete_policy> RETURNING ");
        sql.push_str(&self.descriptor.select_projection());
        sql
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let runtime = self.runtime;
        let descriptor = self.descriptor;
        let mut tx = runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        let (summary, emits_event) =
            run_delete_many_in_tx(&mut tx, runtime, descriptor, &self.filters, ctx).await?;
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        Ok(summary)
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let (summary, _) =
            run_delete_many_in_tx(tx, self.runtime, self.descriptor, &self.filters, ctx).await?;
        Ok(summary)
    }
}
