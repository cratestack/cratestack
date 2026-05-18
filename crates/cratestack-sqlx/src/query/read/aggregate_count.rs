//! `aggregate.count()` — `COUNT(*)` with filter + read policy.

use cratestack_core::{CoolContext, CoolError};
use cratestack_sql::ReadSource;

use crate::query::support::{ReadPolicyKind, push_scoped_conditions};
use crate::{FilterExpr, SqlxRuntime, sqlx};

#[derive(Clone)]
pub struct AggregateCount<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static dyn ReadSource<M, PK>,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateCount<'a, M, PK> {
    pub(super) fn new(
        runtime: &'a SqlxRuntime,
        descriptor: &'static dyn ReadSource<M, PK>,
    ) -> Self {
        Self {
            runtime,
            descriptor,
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
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT COUNT(*) FROM ");
        query.push(self.descriptor.table_name());
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

    pub async fn run(self, ctx: &CoolContext) -> Result<i64, CoolError> {
        let mut query = self.build_query(ctx);
        let value: (i64,) = query
            .build_query_as::<(i64,)>()
            .fetch_one(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(value.0)
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<i64, CoolError> {
        let mut query = self.build_query(ctx);
        let value: (i64,) = query
            .build_query_as::<(i64,)>()
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(value.0)
    }
}
