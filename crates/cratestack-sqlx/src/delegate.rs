use crate::sqlx;

use cratestack_core::{CoolContext, CoolError};

use crate::{
    Aggregate, AggregateColumn, AggregateCount, BatchCreate, BatchDelete, BatchGet, BatchUpdate,
    BatchUpdateItem, BatchUpsert, CreateModelInput, CreateRecord, DeleteMany, DeleteRecord, Filter,
    FilterExpr, FindMany, FindUnique, ModelDescriptor, OrderClause, SqlxRuntime, UpdateMany,
    UpdateManySet, UpdateModelInput, UpdateRecord, UpdateRecordSet, UpsertModelInput, UpsertRecord,
};

#[derive(Debug, Clone, Copy)]
pub struct ModelDelegate<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static ModelDescriptor<M, PK>,
}

impl<'a, M: 'static, PK: 'static> ModelDelegate<'a, M, PK> {
    pub fn new(runtime: &'a SqlxRuntime, descriptor: &'static ModelDescriptor<M, PK>) -> Self {
        Self {
            runtime,
            descriptor,
        }
    }

    pub fn descriptor(&self) -> &'static ModelDescriptor<M, PK> {
        self.descriptor
    }

    pub fn bind(self, ctx: CoolContext) -> ScopedModelDelegate<'a, M, PK> {
        ScopedModelDelegate {
            delegate: self,
            ctx,
        }
    }

    pub fn find_many(&self) -> FindMany<'a, M, PK> {
        FindMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
            order_by: Vec::new(),
            limit: None,
            offset: None,
            for_update: false,
        }
    }

    pub fn find_unique(&self, id: PK) -> FindUnique<'a, M, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
            for_update: false,
            policy_kind: crate::query::ReadPolicyKind::Detail,
        }
    }

    pub fn create<I>(&self, input: I) -> CreateRecord<'a, M, PK, I> {
        CreateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
        }
    }

    /// Insert-or-update on primary-key conflict. Available only on models
    /// whose `@id` field is client-supplied (no `@default(...)`); attempting
    /// to call this on a model with a server-generated PK is a compile error.
    pub fn upsert<I>(&self, input: I) -> UpsertRecord<'a, M, PK, I> {
        UpsertRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
            conflict_target: cratestack_sql::ConflictTarget::PrimaryKey,
        }
    }

    pub fn update(&self, id: PK) -> UpdateRecord<'a, M, PK> {
        UpdateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    /// Bulk UPDATE by predicate. Compose filters via `.where_(...)` on the
    /// returned builder, then supply the patch with `.set(input)`. Refuses
    /// to run without at least one filter — table-wide bulk updates are a
    /// footgun that should be written in raw SQL so the intent is loud at
    /// review time.
    pub fn update_many(&self) -> UpdateMany<'a, M, PK> {
        UpdateMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    pub fn delete(&self, id: PK) -> DeleteRecord<'a, M, PK> {
        DeleteRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    /// Side-effect-free aggregate read. Returns a builder that
    /// branches into `.count()` / `.sum(col)` / `.avg(col)` / `.min(col)`
    /// / `.max(col)`; each branch chains `.where_(...)` filters and
    /// terminates in `.run(ctx)` (or `.run_in_tx(...)`).
    ///
    /// Aggregates apply the read policy AND the soft-delete column,
    /// so the result always describes rows the caller would also be
    /// allowed to retrieve via `find_many`.
    pub fn aggregate(&self) -> Aggregate<'a, M, PK> {
        Aggregate {
            runtime: self.runtime,
            descriptor: self.descriptor,
        }
    }

    /// Bulk DELETE by predicate. Mirrors `update_many` semantically:
    /// applies the delete policy and the soft-delete column (if any),
    /// fans audit + outbox out per-row via RETURNING, and refuses to
    /// run without at least one filter. Returns `BatchSummary` with
    /// `ok = rows_affected`.
    pub fn delete_many(&self) -> DeleteMany<'a, M, PK> {
        DeleteMany {
            runtime: self.runtime,
            descriptor: self.descriptor,
            filters: Vec::new(),
        }
    }

    /// Fetch many rows by primary key in a single round-trip; missing rows
    /// surface as per-item `NotFound` in the envelope rather than aborting.
    pub fn batch_get(&self, ids: Vec<PK>) -> BatchGet<'a, M, PK> {
        BatchGet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert many rows in one outer transaction; each input runs under a
    /// nested SAVEPOINT, so a per-item failure (validation, policy, unique
    /// conflict) doesn't take down the rest of the batch.
    pub fn batch_create<I>(&self, inputs: Vec<I>) -> BatchCreate<'a, M, PK, I> {
        BatchCreate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }

    /// Update many rows in one outer transaction with per-item patches and
    /// optional `if_match` versions. Per-item failures roll back at the
    /// savepoint; successful items commit together.
    pub fn batch_update<I>(
        &self,
        items: Vec<BatchUpdateItem<PK, I>>,
    ) -> BatchUpdate<'a, M, PK, I> {
        BatchUpdate {
            runtime: self.runtime,
            descriptor: self.descriptor,
            items,
        }
    }

    /// Delete many rows by primary key in a single statement; rows that
    /// don't exist (or that policy hid) surface as per-item `NotFound`.
    pub fn batch_delete(&self, ids: Vec<PK>) -> BatchDelete<'a, M, PK> {
        BatchDelete {
            runtime: self.runtime,
            descriptor: self.descriptor,
            ids,
        }
    }

    /// Insert-or-update many rows in one outer transaction with per-item
    /// savepoints. Eligible only for models whose `@id` is client-supplied
    /// — same compile-time gate as the single-row `.upsert(...)`.
    pub fn batch_upsert<I>(&self, inputs: Vec<I>) -> BatchUpsert<'a, M, PK, I> {
        BatchUpsert {
            runtime: self.runtime,
            descriptor: self.descriptor,
            inputs,
        }
    }

    pub async fn authorize_detail(&self, id: PK, ctx: &CoolContext) -> Result<(), CoolError>
    where
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        crate::query::authorize_record_action(
            self.runtime,
            self.descriptor,
            id,
            self.descriptor.detail_allow_policies,
            self.descriptor.detail_deny_policies,
            ctx,
            "detail",
        )
        .await
    }

    pub async fn authorize_update(&self, id: PK, ctx: &CoolContext) -> Result<(), CoolError>
    where
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        crate::query::authorize_record_action(
            self.runtime,
            self.descriptor,
            id,
            self.descriptor.update_allow_policies,
            self.descriptor.update_deny_policies,
            ctx,
            "update",
        )
        .await
    }

    pub async fn authorize_delete(&self, id: PK, ctx: &CoolContext) -> Result<(), CoolError>
    where
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        crate::query::authorize_record_action(
            self.runtime,
            self.descriptor,
            id,
            self.descriptor.delete_allow_policies,
            self.descriptor.delete_deny_policies,
            ctx,
            "delete",
        )
        .await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedModelDelegate<'a, M: 'static, PK: 'static> {
    delegate: ModelDelegate<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedModelDelegate<'a, M, PK> {
    pub fn descriptor(&self) -> &'static ModelDescriptor<M, PK> {
        self.delegate.descriptor()
    }

    pub fn context(&self) -> &CoolContext {
        &self.ctx
    }

    pub fn find_many(&self) -> ScopedFindMany<'a, M, PK> {
        ScopedFindMany {
            request: self.delegate.find_many(),
            ctx: self.ctx.clone(),
        }
    }

    pub fn find_unique(&self, id: PK) -> ScopedFindUnique<'a, M, PK> {
        ScopedFindUnique {
            request: self.delegate.find_unique(id),
            ctx: self.ctx.clone(),
        }
    }

    pub fn create<I>(&self, input: I) -> ScopedCreateRecord<'a, M, PK, I> {
        ScopedCreateRecord {
            request: self.delegate.create(input),
            ctx: self.ctx.clone(),
        }
    }

    pub fn upsert<I>(&self, input: I) -> ScopedUpsertRecord<'a, M, PK, I> {
        ScopedUpsertRecord {
            request: self.delegate.upsert(input),
            ctx: self.ctx.clone(),
        }
    }

    pub fn update(&self, id: PK) -> ScopedUpdateRecord<'a, M, PK> {
        ScopedUpdateRecord {
            request: self.delegate.update(id),
            ctx: self.ctx.clone(),
        }
    }

    pub fn update_many(&self) -> ScopedUpdateMany<'a, M, PK> {
        ScopedUpdateMany {
            request: self.delegate.update_many(),
            ctx: self.ctx.clone(),
        }
    }

    pub fn delete(&self, id: PK) -> ScopedDeleteRecord<'a, M, PK> {
        ScopedDeleteRecord {
            request: self.delegate.delete(id),
            ctx: self.ctx.clone(),
        }
    }

    pub fn delete_many(&self) -> ScopedDeleteMany<'a, M, PK> {
        ScopedDeleteMany {
            request: self.delegate.delete_many(),
            ctx: self.ctx.clone(),
        }
    }

    pub fn aggregate(&self) -> ScopedAggregate<'a, M, PK> {
        ScopedAggregate {
            request: self.delegate.aggregate(),
            ctx: self.ctx.clone(),
        }
    }

    pub fn batch_get(&self, ids: Vec<PK>) -> ScopedBatchGet<'a, M, PK> {
        ScopedBatchGet {
            request: self.delegate.batch_get(ids),
            ctx: self.ctx.clone(),
        }
    }

    pub fn batch_create<I>(&self, inputs: Vec<I>) -> ScopedBatchCreate<'a, M, PK, I> {
        ScopedBatchCreate {
            request: self.delegate.batch_create(inputs),
            ctx: self.ctx.clone(),
        }
    }

    pub fn batch_update<I>(
        &self,
        items: Vec<BatchUpdateItem<PK, I>>,
    ) -> ScopedBatchUpdate<'a, M, PK, I> {
        ScopedBatchUpdate {
            request: self.delegate.batch_update(items),
            ctx: self.ctx.clone(),
        }
    }

    pub fn batch_delete(&self, ids: Vec<PK>) -> ScopedBatchDelete<'a, M, PK> {
        ScopedBatchDelete {
            request: self.delegate.batch_delete(ids),
            ctx: self.ctx.clone(),
        }
    }

    pub fn batch_upsert<I>(&self, inputs: Vec<I>) -> ScopedBatchUpsert<'a, M, PK, I> {
        ScopedBatchUpsert {
            request: self.delegate.batch_upsert(inputs),
            ctx: self.ctx.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedFindMany<'a, M: 'static, PK: 'static> {
    request: FindMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedFindMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    /// See [`FindMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.request = self.request.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.request = self.request.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.request = self.request.offset(offset);
        self
    }

    /// See [`FindMany::for_update`].
    pub fn for_update(mut self) -> Self {
        self.request = self.request.for_update();
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub fn preview_scoped_sql(&self) -> String {
        self.request.preview_scoped_sql(&self.ctx)
    }

    pub async fn run(self) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Vec<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }

    /// See [`FindMany::include`].
    pub fn include<Rel: 'static, RelPK: 'static>(
        self,
        relation: cratestack_sql::RelationInclude<M, Rel, RelPK>,
    ) -> ScopedFindManyWith<'a, M, PK, Rel, RelPK> {
        ScopedFindManyWith {
            request: self.request.include(relation),
            ctx: self.ctx,
        }
    }

    /// See [`FindMany::select`].
    pub fn select<I, C>(self, columns: I) -> ScopedProjectedFindMany<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ScopedProjectedFindMany {
            request: self.request.select(columns),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedProjectedFindMany<'a, M: 'static, PK: 'static> {
    request: crate::ProjectedFindMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedProjectedFindMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.request = self.request.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.request = self.request.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.request = self.request.offset(offset);
        self
    }

    pub async fn run(self) -> Result<Vec<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedFindManyWith<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static> {
    request: crate::FindManyWith<'a, M, PK, Rel, RelPK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, Rel: 'static, RelPK: 'static>
    ScopedFindManyWith<'a, M, PK, Rel, RelPK>
{
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn order_by(mut self, clause: OrderClause) -> Self {
        self.request = self.request.order_by(clause);
        self
    }

    pub fn limit(mut self, limit: i64) -> Self {
        self.request = self.request.limit(limit);
        self
    }

    pub fn offset(mut self, offset: i64) -> Self {
        self.request = self.request.offset(offset);
        self
    }

    pub async fn run(self) -> Result<Vec<(M, Option<Rel>)>, CoolError>
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
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
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
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedFindUnique<'a, M: 'static, PK: 'static> {
    request: FindUnique<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedFindUnique<'a, M, PK> {
    /// See [`FindUnique::for_update`].
    pub fn for_update(mut self) -> Self {
        self.request = self.request.for_update();
        self
    }

    /// See [`FindUnique::as_detail`].
    pub fn as_detail(mut self) -> Self {
        self.request = self.request.as_detail();
        self
    }

    /// See [`FindUnique::as_list`].
    pub fn as_list(mut self) -> Self {
        self.request = self.request.as_list();
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub fn preview_scoped_sql(&self) -> String {
        self.request.preview_scoped_sql(&self.ctx)
    }

    pub async fn run(self) -> Result<Option<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Option<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow>,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }

    /// See [`FindUnique::select`].
    pub fn select<I, C>(self, columns: I) -> ScopedProjectedFindUnique<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: cratestack_sql::IntoColumnName,
    {
        ScopedProjectedFindUnique {
            request: self.request.select(columns),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedProjectedFindUnique<'a, M: 'static, PK: 'static> {
    request: crate::ProjectedFindUnique<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedProjectedFindUnique<'a, M, PK> {
    pub fn as_detail(mut self) -> Self {
        self.request = self.request.as_detail();
        self
    }

    pub fn as_list(mut self) -> Self {
        self.request = self.request.as_list();
        self
    }

    pub fn for_update(mut self) -> Self {
        self.request = self.request.for_update();
        self
    }

    pub async fn run(self) -> Result<Option<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Option<cratestack_sql::Projection<M>>, CoolError>
    where
        M: crate::FromPartialPgRow,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedCreateRecord<'a, M: 'static, PK: 'static, I> {
    request: CreateRecord<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedCreateRecord<'a, M, PK, I>
where
    I: CreateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpsertRecord<'a, M: 'static, PK: 'static, I> {
    request: UpsertRecord<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// See [`UpsertRecord::on_conflict`].
    pub fn on_conflict(mut self, target: cratestack_sql::ConflictTarget) -> Self {
        self.request = self.request.on_conflict(target);
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateRecord<'a, M: 'static, PK: 'static> {
    request: UpdateRecord<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedUpdateRecord<'a, M, PK> {
    pub fn set<I>(self, input: I) -> ScopedUpdateRecordSet<'a, M, PK, I> {
        ScopedUpdateRecordSet {
            request: self.request.set(input),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    request: UpdateRecordSet<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }

    /// Attach an expected version for optimistic locking. See
    /// [`UpdateRecordSet::if_match`].
    pub fn if_match(mut self, expected: i64) -> Self {
        self.request = self.request.if_match(expected);
        self
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateMany<'a, M: 'static, PK: 'static> {
    request: UpdateMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedUpdateMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    /// See [`UpdateMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn set<I>(self, input: I) -> ScopedUpdateManySet<'a, M, PK, I> {
        ScopedUpdateManySet {
            request: self.request.set(input),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedUpdateManySet<'a, M: 'static, PK: 'static, I> {
    request: UpdateManySet<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedUpdateManySet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedDeleteRecord<'a, M: 'static, PK: 'static> {
    request: DeleteRecord<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteRecord<'a, M, PK> {
    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedDeleteMany<'a, M: 'static, PK: 'static> {
    request: DeleteMany<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedDeleteMany<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    /// See [`DeleteMany::where_optional`].
    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub fn preview_sql(&self) -> String {
        self.request.preview_sql()
    }

    pub async fn run(self) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<cratestack_core::BatchSummary, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedAggregate<'a, M: 'static, PK: 'static> {
    request: Aggregate<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregate<'a, M, PK> {
    pub fn count(self) -> ScopedAggregateCount<'a, M, PK> {
        ScopedAggregateCount {
            request: self.request.count(),
            ctx: self.ctx,
        }
    }

    pub fn sum<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.sum(column),
            ctx: self.ctx,
        }
    }

    pub fn avg<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.avg(column),
            ctx: self.ctx,
        }
    }

    pub fn min<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.min(column),
            ctx: self.ctx,
        }
    }

    pub fn max<C: cratestack_sql::IntoColumnName>(
        self,
        column: C,
    ) -> ScopedAggregateColumn<'a, M, PK> {
        ScopedAggregateColumn {
            request: self.request.max(column),
            ctx: self.ctx,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScopedAggregateCount<'a, M: 'static, PK: 'static> {
    request: AggregateCount<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregateCount<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub async fn run(self) -> Result<i64, CoolError> {
        self.request.run(&self.ctx).await
    }

    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<i64, CoolError> {
        self.request.run_in_tx(tx, &self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedAggregateColumn<'a, M: 'static, PK: 'static> {
    request: AggregateColumn<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedAggregateColumn<'a, M, PK> {
    pub fn where_(mut self, filter: Filter) -> Self {
        self.request = self.request.where_(filter);
        self
    }

    pub fn where_expr(mut self, filter: FilterExpr) -> Self {
        self.request = self.request.where_expr(filter);
        self
    }

    pub fn where_any(mut self, filters: impl IntoIterator<Item = FilterExpr>) -> Self {
        self.request = self.request.where_any(filters);
        self
    }

    pub fn where_optional<F>(mut self, filter: Option<F>) -> Self
    where
        F: Into<FilterExpr>,
    {
        self.request = self.request.where_optional(filter);
        self
    }

    pub async fn run<T>(self) -> Result<Option<T>, CoolError>
    where
        T: Send
            + Unpin
            + for<'r> sqlx::Decode<'r, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
    {
        self.request.run::<T>(&self.ctx).await
    }

    pub async fn run_in_tx<'tx, T>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
    ) -> Result<Option<T>, CoolError>
    where
        T: Send
            + Unpin
            + for<'r> sqlx::Decode<'r, sqlx::Postgres>
            + sqlx::Type<sqlx::Postgres>,
    {
        self.request.run_in_tx::<T>(tx, &self.ctx).await
    }
}

// ───── Scoped batch wrappers ────────────────────────────────────────────────
//
// Thin lifetime-and-context shims around the unscoped batch builders. The
// shape mirrors the existing `ScopedCreateRecord` / `ScopedUpdateRecord`
// pairs: capture the request-bound `CoolContext` once at `.bind(ctx)` time,
// thread it into `.run()` automatically.

use std::hash::Hash;

use cratestack_core::BatchResponse;

#[derive(Debug, Clone)]
pub struct ScopedBatchGet<'a, M: 'static, PK: 'static> {
    request: BatchGet<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedBatchGet<'a, M, PK> {
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M:
            Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + crate::ModelPrimaryKey<PK>,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchCreate<'a, M: 'static, PK: 'static, I> {
    request: BatchCreate<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchCreate<'a, M, PK, I>
where
    I: CreateModelInput<M> + Send,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchUpdate<'a, M: 'static, PK: 'static, I> {
    request: BatchUpdate<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpdate<'a, M, PK, I>
where
    I: UpdateModelInput<M> + Send,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchDelete<'a, M: 'static, PK: 'static> {
    request: BatchDelete<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedBatchDelete<'a, M, PK> {
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + crate::ModelPrimaryKey<PK>
            + serde::Serialize,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}

#[derive(Debug, Clone)]
pub struct ScopedBatchUpsert<'a, M: 'static, PK: 'static, I> {
    request: BatchUpsert<'a, M, PK, I>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static, I> ScopedBatchUpsert<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    pub async fn run(self) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        self.request.run(&self.ctx).await
    }
}
