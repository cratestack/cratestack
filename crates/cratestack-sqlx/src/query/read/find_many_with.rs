//! `FindMany` with one to-one relation side-loaded — see
//! [`FindMany::include`]. All builder methods delegate to the parent
//! `FindMany`; only `run` / `run_in_tx` differ to fan out into the
//! side-load step.

use cratestack_core::{CoolContext, CoolError};

use crate::{FilterExpr, OrderClause, sqlx};

use super::find_many::FindMany;
use super::side_load::run_side_load;

#[derive(Clone)]
pub struct FindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    parent: FindMany<'a, M, PK>,
    relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static>
    FindManyWith<'a, M, PK, Rel, RelPK>
{
    pub(super) fn new(
        parent: FindMany<'a, M, PK>,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> Self {
        Self { parent, relation }
    }

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
        for<'r> Rel: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + cratestack_sql::ModelPrimaryKey<RelPK>,
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
        run_side_load(
            runtime,
            &parents,
            relation,
            ctx,
            None::<&mut sqlx::Transaction<'_, sqlx::Postgres>>,
        )
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
        for<'r> Rel: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + cratestack_sql::ModelPrimaryKey<RelPK>,
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
