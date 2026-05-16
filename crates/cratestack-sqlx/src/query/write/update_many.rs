//! Bulk UPDATE-by-predicate: emit one statement that mutates every
//! row the filter matches AND the update policy admits, in one
//! round-trip.
//!
//! Differences from per-row `.update(id).set(input)`:
//!   * No `if_match` slot — bulk updates aren't an optimistic-locking
//!     idiom. `@version` is auto-incremented for every matched row;
//!     the caller does NOT supply an expected version.
//!   * Requires at least one filter — predicate-less bulk updates
//!     should be raw SQL so the intent is obvious at review.

use cratestack_core::{BatchSummary, CoolContext, CoolError};

use crate::{FilterExpr, ModelDescriptor, SqlxRuntime, UpdateModelInput, sqlx};

use super::preview::render_update_many_preview_sql;
use super::update_many_exec::run_update_many_in_tx;

#[derive(Debug, Clone)]
pub struct UpdateMany<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> UpdateMany<'a, M, PK> {
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

    /// Conditionally append a filter — `None` is a no-op.
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    /// Supply the patch values. Returns a builder ready to `.run(ctx)`.
    pub fn set<I>(self, input: I) -> UpdateManySet<'a, M, PK, I> {
        UpdateManySet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            input,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateManySet<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
    pub(crate) input: I,
}

impl<'a, M: 'static, PK: 'static, I> UpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let columns: Vec<&str> = values.iter().map(|v| v.column).collect();
        render_update_many_preview_sql(
            self.descriptor.table_name,
            self.descriptor.soft_delete_column.is_some(),
            self.descriptor.version_column,
            &columns,
            &self.descriptor.select_projection(),
        )
    }

    /// Returns `BatchSummary { total, ok, err }` where
    /// `total = ok = rows actually updated` and `err = 0`.
    /// Statement-level failures surface as the outer `Err`.
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
        let (summary, emits_event) = run_update_many_in_tx(
            &mut tx,
            runtime.pool(),
            descriptor,
            &self.filters,
            self.input,
            ctx,
        )
        .await?;
        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        Ok(summary)
    }

    /// Run inside a caller-supplied transaction. Audit + outbox
    /// writes land in `tx`; caller commits.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        let (summary, _) = run_update_many_in_tx(
            tx,
            self.runtime.pool(),
            self.descriptor,
            &self.filters,
            self.input,
            ctx,
        )
        .await?;
        Ok(summary)
    }
}
