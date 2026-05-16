//! `ModelDelegate` authorize_* preflight probes. Each runs a single
//! `SELECT 1 WHERE policy(...)` to verify the caller may act on a
//! given row before the actual mutation. Used by the generated
//! procedure handlers when they take a `@authorize(Model, action,
//! args.path)` attribute.

use cratestack_core::{CoolContext, CoolError};

use crate::sqlx;

use super::model::ModelDelegate;

impl<'a, M: 'static, PK: 'static> ModelDelegate<'a, M, PK> {
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
