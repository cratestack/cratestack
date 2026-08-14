//! `aggregate.count()` — `COUNT(*)` with filter + read policy.

use cratestack_core::{CratestackContext, CratestackError};
use cratestack_sql::ReadSource;

use crate::query::support::{ReadPolicyKind, push_scoped_conditions};
use crate::{FilterExpr, SqlxRuntime, sqlx};

use super::find_many::FindMany;

#[derive(Clone)]
pub struct AggregateCount<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static dyn ReadSource<M, PK>,
    filters: Vec<FilterExpr>,
}

/// Reuses the exact `filters` a `find_many` builder assembled for a
/// `FindMany`, discarding `order_by`/`limit`/`offset`/`for_update` —
/// meaningless for a scalar `COUNT(*)`. Both `FindMany::run` and
/// `AggregateCount::run` push their WHERE clause through the same
/// [`push_scoped_conditions`] with the same descriptor and
/// [`ReadPolicyKind::List`], so transferring `filters` verbatim (rather
/// than re-deriving them from the caller's query a second time) is what
/// guarantees the count can't apply a different `WHERE` clause or
/// policy scope than the page it describes — see cratestack#570, whose
/// whole risk was exactly that kind of divergence.
impl<'a, M: 'static, PK: 'static> From<FindMany<'a, M, PK>> for AggregateCount<'a, M, PK> {
    fn from(find_many: FindMany<'a, M, PK>) -> Self {
        Self {
            runtime: find_many.runtime,
            descriptor: find_many.descriptor,
            filters: find_many.filters,
        }
    }
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

    fn build_query<'q>(&self, ctx: &CratestackContext) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
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

    /// The exact `COUNT(*)` SQL this would run, without executing it —
    /// built by the same `build_query` that `run`/`run_in_tx` use, so
    /// this can't drift from what actually gets sent (unlike a
    /// hand-rolled preview string-builder). No live DB connection is
    /// required: `QueryBuilder` assembly is pure string/bind-slot
    /// bookkeeping.
    pub fn preview_scoped_sql(&self, ctx: &CratestackContext) -> String {
        self.build_query(ctx).sql().to_owned()
    }

    pub async fn run(self, ctx: &CratestackContext) -> Result<i64, CratestackError> {
        let mut query = self.build_query(ctx);
        let value: (i64,) = query
            .build_query_as::<(i64,)>()
            .fetch_one(self.runtime.pool())
            .await
            .map_err(|error| CratestackError::Database(error.to_string()))?;
        Ok(value.0)
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CratestackContext,
    ) -> Result<i64, CratestackError> {
        let mut query = self.build_query(ctx);
        let value: (i64,) = query
            .build_query_as::<(i64,)>()
            .fetch_one(&mut **tx)
            .await
            .map_err(|error| CratestackError::Database(error.to_string()))?;
        Ok(value.0)
    }
}
