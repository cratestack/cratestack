//! `find_unique` — single-row read by PK with optional `FOR UPDATE`
//! and a List/Detail policy-slot toggle.

use cratestack_core::{CoolContext, CoolError};

use crate::query::support::{ReadPolicyKind, push_scoped_conditions};
use crate::render::render_read_policy_sql;
use crate::{ModelDescriptor, SqlxRuntime, sqlx};

#[derive(Debug, Clone)]
pub struct FindUnique<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
    pub(crate) for_update: bool,
    pub(crate) policy_kind: ReadPolicyKind,
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK> {
    /// Emit `SELECT ... FOR UPDATE`. See `FindMany::for_update` for
    /// the tx-pairing caveat.
    pub fn for_update(mut self) -> Self {
        self.for_update = true;
        self
    }

    /// Evaluate against the schema's `detail` policy slice (the
    /// default for `find_unique`). A no-op when called explicitly,
    /// kept for API symmetry with [`Self::as_list`].
    pub fn as_detail(mut self) -> Self {
        self.policy_kind = ReadPolicyKind::Detail;
        self
    }

    /// Evaluate against the schema's `read`/`list` policy slice
    /// instead of `detail`. Use when the call site needs list-style
    /// permission semantics on a unique-key lookup.
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
