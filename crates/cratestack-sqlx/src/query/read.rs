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

    /// Conditionally append a filter. `None` is a no-op so callers
    /// can pipe `FieldRef::match_optional(...)` results straight in
    /// without an `if let` ladder at every site that handles
    /// optional query parameters.
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

    /// Side-load a to-one relation alongside the matched rows.
    /// Returns a builder whose `.run` resolves to
    /// `Vec<(M, Option<Rel>)>` — the option captures both
    /// nullable-FK rows and rows whose related target was filtered out
    /// by the related model's read policy.
    ///
    /// Implementation strategy: two queries, not a SQL JOIN. The
    /// first round-trip materializes the parent rows under the
    /// caller's filters; the second round-trip loads the related
    /// rows with `target_pk IN (collected FKs)`, applying the
    /// related model's read policy + soft-delete just like a normal
    /// `find_many` would. We then merge in-memory keyed by the
    /// extracted FK.
    ///
    /// Why not a SQL JOIN? Two reasons. (1) Aliasing `SELECT
    /// parent.*, related.*` collides on overlapping column names
    /// (`id` is universal) and forces every model's `FromRow` impl
    /// to learn a join-aware projection — a deeper codegen change.
    /// (2) The two-query path naturally inherits the related-side
    /// read policy via `find_many`, so policy correctness costs us
    /// nothing extra. The cost is one extra round-trip per
    /// `.include()`; for the named upstream site (delivery batcher)
    /// the cardinalities make this trivially acceptable.
    pub fn include<Rel, RelPK>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> FindManyWith<'a, M, PK, Rel, RelPK>
    where
        Rel: 'static,
        RelPK: 'static,
    {
        FindManyWith {
            parent: self,
            relation,
        }
    }
}

/// `FindMany` with one to-one relation side-loaded — see
/// [`FindMany::include`].
#[derive(Debug, Clone)]
pub struct FindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    parent: FindMany<'a, M, PK>,
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> FindManyWith<'a, M, PK, Rel, RelPK> {
    pub fn where_(mut self, filter: crate::Filter) -> Self {
        self.parent = self.parent.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.parent = self.parent.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.parent = self.parent.where_any(filters);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.parent = self.parent.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.parent = self.parent.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.parent = self.parent.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.parent = self.parent.offset(offset);
        self
    }

    /// Apply `FOR UPDATE` to the parent-row SELECT. The related-side
    /// side-load query is **not** locked — to lock both sides, wrap
    /// the call in [`Self::run_in_tx`] and issue an explicit
    /// `SELECT ... FOR UPDATE` against the related table separately.
    pub fn for_update(mut self) -> Self {
        self.parent = self.parent.for_update();
        self
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<Vec<(M, Option<Rel>)>, CoolError>
    where
        M: Clone,
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        Rel: Clone,
        for<'r> Rel:
            Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Send
            + Clone
            + std::cmp::Eq
            + std::hash::Hash
            + cratestack_sql::IntoSqlValue
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run(ctx).await?;
        run_side_load(runtime, &parents, relation, ctx, None::<&mut sqlx::Transaction<'_, sqlx::Postgres>>)
            .await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Vec<(M, Option<Rel>)>, CoolError>
    where
        M: Clone,
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        Rel: Clone,
        for<'r> Rel:
            Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + cratestack_sql::ModelPrimaryKey<RelPK>,
        RelPK: Send
            + Clone
            + std::cmp::Eq
            + std::hash::Hash
            + cratestack_sql::IntoSqlValue
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let runtime = self.parent.runtime;
        let relation = self.relation;
        let parents = self.parent.run_in_tx(tx, ctx).await?;
        run_side_load(runtime, &parents, relation, ctx, Some(tx)).await
    }
}

async fn run_side_load<'tx, M, Rel, RelPK>(
    runtime: &SqlxRuntime,
    parents: &[M],
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ctx: &CoolContext,
    tx: Option<&mut sqlx::Transaction<'tx, sqlx::Postgres>>,
) -> Result<Vec<(M, Option<Rel>)>, CoolError>
where
    M: 'static + Clone,
    for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    Rel: 'static + Clone,
    for<'r> Rel:
        Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + cratestack_sql::ModelPrimaryKey<RelPK>,
    RelPK: 'static
        + Send
        + Clone
        + std::cmp::Eq
        + std::hash::Hash
        + cratestack_sql::IntoSqlValue
        + sqlx::Type<sqlx::Postgres>
        + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
{
    // Collect distinct non-null FK values from the parent rows. Dedup
    // preserves the per-parent merge step but trims the IN-list — for
    // batchy dispatchers where many parents share a target subscription,
    // this can collapse the side-load to a single matched row.
    let mut fk_values: Vec<RelPK> = Vec::new();
    let mut seen: std::collections::HashSet<RelPK> = std::collections::HashSet::new();
    for parent in parents {
        if let Some(fk) = (relation.parent_fk_extract)(parent)
            && seen.insert(fk.clone())
        {
            fk_values.push(fk);
        }
    }

    let by_pk: std::collections::HashMap<RelPK, Rel> = if fk_values.is_empty() {
        std::collections::HashMap::new()
    } else {
        // The side-load runs under the same read policy as a normal
        // find_many on the related descriptor, so rows the caller
        // can't see drop out and surface as `None` on the merged
        // side. Reusing FindMany also picks up soft-delete + the
        // standard order_by tie-breaker for free.
        let primary_key = relation.related_descriptor.primary_key;
        let related_rows: Vec<Rel> = {
            let mut probe = FindMany {
                runtime,
                descriptor: relation.related_descriptor,
                filters: Vec::new(),
                order_by: Vec::new(),
                limit: None,
                offset: None,
                for_update: false,
            };
            // Build the IN filter through the typed filter AST so the
            // bind values flow through `push_bind_value` like every
            // other read query.
            probe.filters.push(FilterExpr::from(crate::Filter {
                column: primary_key,
                op: cratestack_sql::FilterOp::In,
                value: cratestack_sql::FilterValue::Many(
                    fk_values
                        .iter()
                        .cloned()
                        .map(cratestack_sql::IntoSqlValue::into_sql_value)
                        .collect(),
                ),
            }));
            match tx {
                Some(tx) => probe.run_in_tx(tx, ctx).await?,
                None => probe.run(ctx).await?,
            }
        };
        related_rows
            .into_iter()
            .map(|r| {
                let pk = cratestack_sql::ModelPrimaryKey::primary_key(&r);
                (pk, r)
            })
            .collect()
    };

    Ok(parents
        .iter()
        .map(|m| {
            let related = (relation.parent_fk_extract)(m)
                .and_then(|fk| by_pk.get(&fk))
                .cloned();
            (m.clone(), related)
        })
        .collect())
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

// ───── Aggregate ───────────────────────────────────────────────────────────
//
// Side-effect-free read query that returns a single scalar (or a few
// scalars worth of stats) about the rows matching the filter set.
//
// Like `find_many`, the result is filtered through the model's read
// policy AND the soft-delete column — aggregate counts could otherwise
// leak privileged information ("this admin has 100 sensitive records"
// when the caller can't see any of them). Aggregates are evaluated
// post-filter, so the count/sum/avg/etc. always describe rows the
// caller would be allowed to retrieve via `find_many`.
//
// Each operation returns a dedicated builder rather than a single
// polymorphic struct because the return type genuinely differs: count
// is `i64` (never NULL — empty match means 0), but sum / avg / min /
// max are `Option<T>` (NULL when no rows match). The dedicated types
// also let the schema studio generate ergonomic call sites without
// resorting to turbofish.

use cratestack_sql::IntoColumnName;

#[derive(Debug, Clone)]
pub struct Aggregate<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> Aggregate<'a, M, PK> {
    /// `COUNT(*)` over the matching rows. Always returns a concrete
    /// `i64`; empty matches yield 0 rather than NULL.
    pub fn count(self) -> AggregateCount<'a, M, PK> {
        AggregateCount {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    /// `SUM(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn sum<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Sum, column)
    }

    /// `AVG(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn avg<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Avg, column)
    }

    /// `MIN(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn min<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Min, column)
    }

    /// `MAX(<col>)`. Returns `Option<T>` — `None` when no rows match.
    pub fn max<C: IntoColumnName>(self, column: C) -> AggregateColumn<'a, M, PK> {
        AggregateColumn::new(self.runtime, self.descriptor, AggregateOp::Max, column)
    }
}

#[derive(Debug, Clone, Copy)]
enum AggregateOp {
    Sum,
    Avg,
    Min,
    Max,
}

impl AggregateOp {
    fn function_name(self) -> &'static str {
        match self {
            Self::Sum => "SUM",
            Self::Avg => "AVG",
            Self::Min => "MIN",
            Self::Max => "MAX",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AggregateCount<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateCount<'a, M, PK> {
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

    fn build_query<'q>(
        &self,
        ctx: &CoolContext,
    ) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT COUNT(*) FROM ");
        query.push(self.descriptor.table_name);
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

#[derive(Debug, Clone)]
pub struct AggregateColumn<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    op: AggregateOp,
    column: &'static str,
    filters: Vec<FilterExpr>,
}

impl<'a, M: 'static, PK: 'static> AggregateColumn<'a, M, PK> {
    fn new<C: IntoColumnName>(
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

    fn build_query<'q>(
        &self,
        ctx: &CoolContext,
    ) -> sqlx::QueryBuilder<'q, sqlx::Postgres> {
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

    /// Run the aggregate. `T` is whatever sqlx scalar type decodes the
    /// expression result — for PG `SUM(int)` that's `i64`, for `AVG(int)`
    /// it's `f64` or `rust_decimal::Decimal`, for `MIN/MAX(timestamp)`
    /// it's `chrono::DateTime<Utc>`, etc. Pick at the call site.
    pub async fn run<T>(self, ctx: &CoolContext) -> Result<Option<T>, CoolError>
    where
        T: Send
            + Unpin
            + for<'r> sqlx::Decode<'r, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
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
        T: Send
            + Unpin
            + for<'r> sqlx::Decode<'r, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
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

// ───── Column projection (.select) ─────────────────────────────────────────
//
// `find_unique(id).select([...])` and `find_many().select([...])` emit a
// SQL `SELECT` clause restricted to the named columns and decode the row
// into a `Projection<T>` where non-selected fields hold
// `Default::default()` values. See `cratestack_sql::Projection` for the
// caller-side contract.
//
// Implementation: the projected builders reuse the parent `FindMany` /
// `FindUnique` filter + ordering machinery via `push_scoped_conditions`,
// swapping the projection on the SELECT for `select_projection_subset`
// and routing the decode through the `FromPartialPgRow` impl the macro
// emits per model.

use cratestack_sql::IntoColumnName as _ProjIntoColumnName;

#[derive(Debug, Clone)]
pub struct ProjectedFindUnique<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
    id: PK,
    selected: Vec<&'static str>,
    policy_kind: ReadPolicyKind,
    for_update: bool,
}

impl<'a, M: 'static, PK: 'static> ProjectedFindUnique<'a, M, PK> {
    pub fn as_detail(mut self) -> Self {
        self.policy_kind = ReadPolicyKind::Detail;
        self
    }

    pub fn as_list(mut self) -> Self {
        self.policy_kind = ReadPolicyKind::List;
        self
    }

    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    pub async fn run(
        self,
        ctx: &CoolContext,
    ) -> Result<Option<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection_subset(&self.selected))
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

        let row = query
            .build()
            .fetch_optional(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        match row {
            Some(row) => {
                let value = M::decode_partial_pg_row(&row, &self.selected)
                    .map_err(|error| CoolError::Database(error.to_string()))?;
                Ok(Some(cratestack_sql::Projection {
                    value,
                    selected: self.selected,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Option<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query
            .push(self.descriptor.select_projection_subset(&self.selected))
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

        let row = query
            .build()
            .fetch_optional(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        match row {
            Some(row) => {
                let value = M::decode_partial_pg_row(&row, &self.selected)
                    .map_err(|error| CoolError::Database(error.to_string()))?;
                Ok(Some(cratestack_sql::Projection {
                    value,
                    selected: self.selected,
                }))
            }
            None => Ok(None),
        }
    }
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK> {
    /// Restrict the SELECT to the named columns. The returned builder
    /// resolves to `Option<Projection<M>>` rather than `Option<M>`;
    /// non-selected fields on the inner `M` hold `Default::default()`
    /// values, so callers should check `Projection::is_selected(col)`
    /// before reading a field they didn't request.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindUnique<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: _ProjIntoColumnName,
    {
        ProjectedFindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            selected: columns
                .into_iter()
                .map(_ProjIntoColumnName::into_column_name)
                .collect(),
            policy_kind: self.policy_kind,
            for_update: self.for_update,
        }
    }
}

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

    pub async fn run(
        self,
        ctx: &CoolContext,
    ) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
    {
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

        let rows = query
            .build()
            .fetch_all(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        rows.into_iter()
            .map(|row| {
                M::decode_partial_pg_row(&row, &self.selected)
                    .map(|value| cratestack_sql::Projection {
                        value,
                        selected: self.selected.clone(),
                    })
                    .map_err(|error| CoolError::Database(error.to_string()))
            })
            .collect()
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
    {
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

        let rows = query
            .build()
            .fetch_all(&mut **tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        rows.into_iter()
            .map(|row| {
                M::decode_partial_pg_row(&row, &self.selected)
                    .map(|value| cratestack_sql::Projection {
                        value,
                        selected: self.selected.clone(),
                    })
                    .map_err(|error| CoolError::Database(error.to_string()))
            })
            .collect()
    }
}

impl<'a, M: 'static, PK: 'static> FindMany<'a, M, PK> {
    /// Restrict the SELECT to the named columns. See
    /// [`FindUnique::select`] for the caller-side contract.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: _ProjIntoColumnName,
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
                .map(_ProjIntoColumnName::into_column_name)
                .collect(),
        }
    }
}
