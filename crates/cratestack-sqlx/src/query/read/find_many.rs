//! `find_many` — typed multi-row read with filter / order /
//! pagination / `FOR UPDATE`. The `preview_*_sql` previews live in
//! [`super::find_many_preview`] to keep this file under budget.

use cratestack_core::{CoolContext, CoolError};
use cratestack_sql::ReadSource;

use crate::query::support::{ReadPolicyKind, push_order_and_paging, push_scoped_conditions};
use crate::{FilterExpr, OrderClause, SqlxRuntime, sqlx};

#[derive(Clone)]
pub struct FindMany<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    /// Either a `&'static ModelDescriptor<M, PK>` (typical) or a
    /// `&'static ViewDescriptor<M, PK>` (view path). Both impl
    /// `ReadSource<M, PK>`.
    pub(crate) descriptor: &'static dyn ReadSource<M, PK>,
    pub(crate) filters: Vec<FilterExpr>,
    pub(crate) order_by: Vec<OrderClause>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
    pub(crate) for_update: bool,
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
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

    /// Conditionally append a filter. `None` is a no-op so callers can
    /// pipe `FieldRef::match_optional(...)` results straight in
    /// without an `if let` ladder at every optional-param site.
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        if let Some(filter) = filter {
            self.filters.push(filter.into());
        }
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.order_by.push(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Emit `SELECT ... FOR UPDATE` so the engine takes an exclusive
    /// row-level lock on every matched row for the surrounding
    /// transaction. Only meaningful when paired with [`Self::run_in_tx`].
    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    pub fn preview_sql(&self) -> String {
        super::find_many_preview::preview_sql(self)
    }

    pub fn preview_scoped_sql(&self, ctx: &CoolContext) -> String {
        super::find_many_preview::preview_scoped_sql(self, ctx)
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        let order_by = self.effective_order_by();
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection())
            .push(" FROM ")
            .push(self.descriptor.table_name());

        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &self.filters,
            None::<(&'static str, i64)>,
            ctx,
            ReadPolicyKind::List,
        );
        push_order_and_paging(&mut query, &order_by, self.limit, self.offset);
        if self.for_update {
            query.push(" FOR UPDATE");
        }

        query
            .build_query_as::<M>()
            .fetch_all(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))
    }

    /// Run inside a caller-supplied transaction. Required when pairing
    /// with [`Self::for_update`].
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        let order_by = self.effective_order_by();
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection())
            .push(" FROM ")
            .push(self.descriptor.table_name());

        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &self.filters,
            None::<(&'static str, i64)>,
            ctx,
            ReadPolicyKind::List,
        );
        push_order_and_paging(&mut query, &order_by, self.limit, self.offset);
        if self.for_update {
            query.push(" FOR UPDATE");
        }

        query
            .build_query_as::<M>()
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))
    }

    pub(super) fn effective_order_by(&self) -> Vec<OrderClause> {
        let mut order_by = self.order_by.clone();
        let Some(direction) = order_by
            .iter()
            .find(|clause| clause.is_relation_scalar())
            .map(OrderClause::direction)
        else {
            return order_by;
        };

        if order_by
            .iter()
            .any(|clause| clause.targets_column(self.descriptor.primary_key()))
        {
            return order_by;
        }

        order_by.push(OrderClause::column(self.descriptor.primary_key(), direction));
        order_by
    }

    /// Side-load a to-one relation alongside the matched rows. Two
    /// queries, not a SQL JOIN, so the related-side read policy +
    /// soft-delete inherit from `find_many` for free.
    pub fn include<Rel, RelPK>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> super::find_many_with::FindManyWith<'a, M, PK, Rel, RelPK>
    where
        Rel: 'static,
        RelPK: 'static,
    {
        super::find_many_with::FindManyWith::new(self, relation)
    }
}
