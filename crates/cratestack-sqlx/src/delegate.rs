use cratestack_core::{CoolContext, CoolError};

use crate::{
    CreateModelInput, CreateRecord, DeleteRecord, Filter, FilterExpr, FindMany, FindUnique,
    ModelDescriptor, OrderClause, SqlxRuntime, UpdateModelInput, UpdateRecord, UpdateRecordSet,
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
        }
    }

    pub fn find_unique(&self, id: PK) -> FindUnique<'a, M, PK> {
        FindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    pub fn create<I>(&self, input: I) -> CreateRecord<'a, M, PK, I> {
        CreateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input,
        }
    }

    pub fn update(&self, id: PK) -> UpdateRecord<'a, M, PK> {
        UpdateRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
        }
    }

    pub fn delete(&self, id: PK) -> DeleteRecord<'a, M, PK> {
        DeleteRecord {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id,
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
            self.descriptor.auth.detail_allow_policies,
            self.descriptor.auth.detail_deny_policies,
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
            self.descriptor.auth.update_allow_policies,
            self.descriptor.auth.update_deny_policies,
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
            self.descriptor.auth.delete_allow_policies,
            self.descriptor.auth.delete_deny_policies,
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

    pub fn update(&self, id: PK) -> ScopedUpdateRecord<'a, M, PK> {
        ScopedUpdateRecord {
            request: self.delegate.update(id),
            ctx: self.ctx.clone(),
        }
    }

    pub fn delete(&self, id: PK) -> ScopedDeleteRecord<'a, M, PK> {
        ScopedDeleteRecord {
            request: self.delegate.delete(id),
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
}

#[derive(Debug, Clone)]
pub struct ScopedFindUnique<'a, M: 'static, PK: 'static> {
    request: FindUnique<'a, M, PK>,
    ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedFindUnique<'a, M, PK> {
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

    /// Attach an expected version for optimistic locking. See
    /// [`UpdateRecordSet::if_match`].
    pub fn if_match(mut self, expected: i64) -> Self {
        self.request = self.request.if_match(expected);
        self
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
}
