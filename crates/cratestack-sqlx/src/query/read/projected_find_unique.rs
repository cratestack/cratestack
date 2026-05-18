//! `find_unique(...).select([...])` — projected single-row read that
//! returns `Option<Projection<M>>`. Non-selected fields hold
//! `Default::default()` values; callers gate reads on
//! `Projection::is_selected(col)`.

use cratestack_core::{CoolContext, CoolError};
use cratestack_sql::{IntoColumnName, ReadSource};

use crate::query::support::{ReadPolicyKind, push_scoped_conditions};
use crate::{SqlxRuntime, sqlx};

use super::find_unique::FindUnique;

#[derive(Clone)]
pub struct ProjectedFindUnique<'a, M: 'static, PK: 'static> {
    runtime: &'a SqlxRuntime,
    descriptor: &'static dyn ReadSource<M, PK>,
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
            .push(self.descriptor.table_name());
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &[],
            Some((self.descriptor.primary_key(), self.id)),
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
        decode_optional(row, &self.selected)
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
            .push(self.descriptor.table_name());
        push_scoped_conditions(
            &mut query,
            self.descriptor,
            &[],
            Some((self.descriptor.primary_key(), self.id)),
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
        decode_optional(row, &self.selected)
    }
}

fn decode_optional<M>(
    row: Option<sqlx::postgres::PgRow>,
    selected: &[&'static str],
) -> Result<Option<cratestack_sql::Projection<M>>, CoolError>
where
    M: crate::FromPartialPgRow,
{
    match row {
        Some(row) => {
            let value = M::decode_partial_pg_row(&row, selected)
                .map_err(|error| CoolError::Database(error.to_string()))?;
            Ok(Some(cratestack_sql::Projection {
                value,
                selected: selected.to_vec(),
            }))
        }
        None => Ok(None),
    }
}

impl<'a, M: 'static, PK: 'static> FindUnique<'a, M, PK> {
    /// Restrict the SELECT to the named columns. Resolves to
    /// `Option<Projection<M>>` rather than `Option<M>`; non-selected
    /// fields on the inner `M` hold `Default::default()`.
    pub fn select<I, C>(self, columns: I) -> ProjectedFindUnique<'a, M, PK>
    where
        I: IntoIterator<Item = C>,
        C: IntoColumnName,
    {
        ProjectedFindUnique {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            selected: columns
                .into_iter()
                .map(IntoColumnName::into_column_name)
                .collect(),
            policy_kind: self.policy_kind,
            for_update: self.for_update,
        }
    }
}
