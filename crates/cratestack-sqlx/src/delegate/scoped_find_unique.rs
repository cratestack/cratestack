//! `ScopedFindUnique` + the projected exit `ScopedProjectedFindUnique`.

use cratestack_core::{CoolContext, CoolError};

use crate::{FindUnique, sqlx};

#[derive(Clone)]
pub struct ScopedFindUnique<'a, M: 'static, PK: 'static> {
    pub(super) request: FindUnique<'a, M, PK>,
    pub(super) ctx: CoolContext,
}

impl<'a, M: 'static, PK: 'static> ScopedFindUnique<'a, M, PK> {
    pub(super) fn new(request: FindUnique<'a, M, PK>, ctx: CoolContext) -> Self {
        Self { request, ctx }
    }

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

#[derive(Clone)]
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
