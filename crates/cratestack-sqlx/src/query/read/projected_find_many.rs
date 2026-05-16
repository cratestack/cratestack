//! `find_many().select([...])` — projected multi-row read that
//! returns `Vec<Projection<M>>`. Same partial-decode contract as
//! [`super::projected_find_unique`].

use cratestack_core::{CoolContext, CoolError};
use cratestack_sql::IntoColumnName;

use crate::query::support::{ReadPolicyKind, push_order_and_paging, push_scoped_conditions};
use crate::{FilterExpr, ModelDescriptor, OrderClause, SqlxRuntime, sqlx};

use super::find_many::FindMany;

#[derive(Debug, Clone)]
pub struct ProjectedFindMany<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
    order_by: Vec<OrderClause>,
    limit: Option<i64>,
    offset: Option<i64>,
    for_update: bool,
    selected: Vec<&'static str>,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindMany<'a, M, PK> {
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

    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    fn build_query<'q>(&self, ctx: &CoolContext) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection_subset(&self.selected))
            .push(" FROM ")
            .push(self.descriptor.table_name);
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &self.filters,
            None::<(&'static str, i64)>,
            ctx,
            ReadPolicyKind::List,
        );
        push_order_and_paging(&mut query, &self.order_by, self.limit, self.offset);
        if self.for_update {
            query.push(" FOR UPDATE");
        }
        query
    }

    pub async fn run(
        self,
        ctx: &CoolContext,
    ) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
    {
        let mut query = self.build_query(ctx);
        let rows = query
            .build()
            .fetch_all(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        decode_many::<M>(rows, &self.selected)
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
    {
        let mut query = self.build_query(ctx);
        let rows = query
            .build()
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        decode_many::<M>(rows, &self.selected)
    }
}

fn decode_many<M>(
    rows: Vec<sqlx::postgres::PgRow>,
    selected: &[&'static str],
) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
where
    M: crate::FromPartialPgRow,
{
    rows.into_iter()
        .map(|row| {
            M::decode_partial_pg_row(&row, selected)
                .map(|value| cratestack_sql::Projection {
                    value,
                    selected: selected.to_vec(),
                })
                .map_err(|error| CoolError::Database(error.to_string()))
        })
        .collect()
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    /// Restrict the SELECT to the named columns. See
    /// [`super::find_unique::FindUnique::select`] for the caller-side
    /// contract.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: IntoColumnName,
    {
        ProjectedFindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: self.filters,
            order_by: self.order_by,
            limit: self.limit,
            offset: self.offset,
            for_update: self.for_update,
            selected: columns
                .into_iter()
                .map(IntoColumnName::into_column_name)
                .collect(),
        }
    }
}
