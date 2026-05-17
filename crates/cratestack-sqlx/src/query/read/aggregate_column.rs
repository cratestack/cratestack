//! `aggregate.sum/avg/min/max(col)` — single-column scalar aggregates
//! with filter + read policy. Caller picks the decode type at the
//! call site since PG's `SUM(int)` returns i64, `AVG(int)` returns
//! f64/Decimal, etc.

use cratestack_core::{CoolContext, CoolError};
use cratestack_sql::IntoColumnName;

use crate::query::support::{ReadPolicyKind, push_scoped_conditions};
use crate::{FilterExpr, ModelDescriptor, SqlxRuntime, sqlx};

use super::aggregate::AggregateOp;

#[derive(Debug, Clone)]
pub struct AggregateColumn<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    op: AggregateOp,
    column: &'static str,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateColumn<'a, M, PK> {
    pub(super) fn new<C: IntoColumnName>(
        runtime: &'a SqlxRuntime,
        descriptor: &'static ModelDescriptor<M, PK>,
        op: AggregateOp,
        column: C,
    ) -> Self {
        Self {
            runtime,
            descriptor,
            op,
            column: column.into_column_name(),
            filters: Vec::new(),
        }
    }

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

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    fn build_query<'q>(&self, ctx: &CoolContext) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.op.function_name())
            .push("(")
            .push(self.column)
            .push(") FROM ")
            .push(self.descriptor.table_name);
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &self.filters,
            None::<(&'static str, i64)>,
            ctx,
            ReadPolicyKind::List,
        );
        query
    }

    pub async fn run<T>(self, ctx: &CoolContext) -> Result<Option<T>, CoolError>
    where
        T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        let mut query = self.build_query(ctx);
        let value: (Option<T>,) = query
            .build_query_as::<(Option<T>,)>()
            .fetch_one(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(value.0)
    }

    pub async fn run_in_tx<'tx, T>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Option<T>, CoolError>
    where
        T: Send + Unpin + for<'r> sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
    {
        let mut query = self.build_query(ctx);
        let value: (Option<T>,) = query
            .build_query_as::<(Option<T>,)>()
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(value.0)
    }
}
