use crate::sqlx;

use cratestack_core::{CoolContext, CoolError};

use crate::{
    FilterExpr, ModelDescriptor, OrderClause, SqlxRuntime, render::render_read_policy_sql,
    render::render_scoped_select_sql,
};

use super::support::{push_order_and_paging, push_scoped_conditions, ReadPolicyKind};

#[derive(Debug, Clone)]
pub struct FindMany<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
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
    /// row-level lock on every matched row for the duration of the
    /// surrounding transaction. Only meaningful when paired with
    /// [`Self::run_in_tx`] — outside an explicit transaction the lock is
    /// released immediately after the statement and the flag becomes a
    /// no-op (PG accepts the syntax but the lock has nothing to outlive).
    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    pub fn preview_sql(&self) -> String {
        let mut sql = format!(
            "SELECT {} FROM {}",
            self.descriptor.select_projection(),
            self.descriptor.table_name,
        );
        let order_by = self.effective_order_by();

        let mut bind_index = 1usize;
        if !self.filters.is_empty() {
            sql.push_str(" WHERE ");
            for (index, filter) in self.filters.iter().enumerate() {
                if index > 0 {
                    sql.push_str(" AND ");
                }
                crate::render::render_filter_expr_sql(filter, &mut sql, &mut bind_index);
            }
        }

        if !order_by.is_empty() {
            sql.push_str(" ORDER BY ");
            for (index, clause) in order_by.iter().enumerate() {
                if index > 0 {
                    sql.push_str(", ");
                }
                crate::render::render_order_clause_sql(clause, &mut sql);
            }
        }

        match (self.limit, self.offset) {
            (Some(_), Some(_)) => {
                sql.push_str(&format!(" LIMIT ${bind_index} OFFSET ${}", bind_index + 1));
            }
            (Some(_), None) => {
                sql.push_str(&format!(" LIMIT ${bind_index}"));
            }
            (None, Some(_)) => {
                sql.push_str(&format!(" OFFSET ${bind_index}"));
            }
            (None, None) => {}
        }

        if self.for_update {
            sql.push_str(" FOR UPDATE");
        }

        sql
    }

    pub fn preview_scoped_sql(&self, ctx: &CoolContext) -> String {
        let order_by = self.effective_order_by();
        render_scoped_select_sql(
            self.descriptor,
            &self.filters,
            &order_by,
            self.limit,
            self.offset,
            ctx,
        )
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
            .push(self.descriptor.table_name);

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
    /// with [`Self::for_update`] — the row locks only persist for the life
    /// of the transaction, so calling against the pool would emit
    /// `FOR UPDATE` to no effect.
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
            .push(self.descriptor.table_name);

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

    fn effective_order_by(&self) -> Vec<OrderClause> {
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
            .any(|clause| clause.targets_column(self.descriptor.primary_key))
        {
            return order_by;
        }

        order_by.push(OrderClause::column(self.descriptor.primary_key, direction));
        order_by
    }
}

#[derive(Debug, Clone)]
pub struct FindUnique<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
    pub(crate) for_update: bool,
    pub(crate) policy_kind: ReadPolicyKind,
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK> {
    /// Emit `SELECT ... FOR UPDATE` so the engine takes an exclusive
    /// row-level lock on the matched row for the duration of the
    /// surrounding transaction. See [`FindMany::for_update`] for the
    /// tx-pairing caveat.
    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    /// Evaluate against the schema's `detail` policy slice (the default
    /// for `find_unique`). A no-op when called explicitly, kept for
    /// API symmetry with [`Self::as_list`] so call sites can be
    /// self-documenting about which policy slot they want.
    ///
    /// `@@allow("detail", ...)` rules are typically more permissive
    /// than `@@allow("list", ...)` — e.g. "anyone can fetch a public
    /// post by id, but only members can see the listing" — and the
    /// schema author's intent for unique lookups belongs in `detail`.
    pub fn as_detail(mut self) -> Self {
        self.policy_kind = ReadPolicyKind::Detail;
        self
    }

    /// Evaluate against the schema's `read`/`list` policy slice instead
    /// of `detail`. Use when the call site needs list-style permission
    /// semantics on what happens to be a unique-key lookup — most
    /// commonly during a migration from a list-shaped route to a
    /// by-id route that should still preserve the old gate.
    pub fn as_list(mut self) -> Self {
        self.policy_kind = ReadPolicyKind::List;
        self
    }

    pub fn preview_sql(&self) -> String {
        let mut sql = format!(
            "SELECT {} FROM {} WHERE {} = $1 LIMIT 1",
            self.descriptor.select_projection(),
            self.descriptor.table_name,
            self.descriptor.primary_key,
        );
        if self.for_update {
            sql.push_str(" FOR UPDATE");
        }
        sql
    }

    pub fn preview_scoped_sql(&self, ctx: &CoolContext) -> String {
        let mut sql = format!(
            "SELECT {} FROM {}",
            self.descriptor.select_projection(),
            self.descriptor.table_name,
        );
        let mut bind_index = 1usize;
        let (allow, deny) = match self.policy_kind {
            ReadPolicyKind::List => (
                self.descriptor.read_allow_policies,
                self.descriptor.read_deny_policies,
            ),
            ReadPolicyKind::Detail => (
                self.descriptor.detail_allow_policies,
                self.descriptor.detail_deny_policies,
            ),
        };
        if let Some(policy_clause) = render_read_policy_sql(allow, deny, ctx, &mut bind_index) {
            sql.push_str(&format!(
                " WHERE ({policy_clause}) AND {} = ${bind_index} LIMIT 1",
                self.descriptor.primary_key
            ));
        } else {
            sql.push_str(&format!(
                " WHERE {} = ${bind_index} LIMIT 1",
                self.descriptor.primary_key
            ));
        }
        if self.for_update {
            sql.push_str(" FOR UPDATE");
        }
        sql
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<Option<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection())
            .push(" FROM ")
            .push(self.descriptor.table_name);
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &[],
            Some((self.descriptor.primary_key, self.id)),
            ctx,
            self.policy_kind,
        );
        query.push(" LIMIT 1");
        if self.for_update {
            query.push(" FOR UPDATE");
        }

        query
            .build_query_as::<M>()
            .fetch_optional(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))
    }

    /// Run inside a caller-supplied transaction. Required when pairing
    /// with [`Self::for_update`].
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Option<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection())
            .push(" FROM ")
            .push(self.descriptor.table_name);
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &[],
            Some((self.descriptor.primary_key, self.id)),
            ctx,
            self.policy_kind,
        );
        query.push(" LIMIT 1");
        if self.for_update {
            query.push(" FOR UPDATE");
        }

        query
            .build_query_as::<M>()
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))
    }
}
