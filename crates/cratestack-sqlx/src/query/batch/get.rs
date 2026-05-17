//! `batch_get` — single `SELECT ... WHERE pk IN (...) AND policy(...)`.
//! Walk-and-match the returned rows back to input positions; absent
//! PKs surface as `NotFound`.

use std::collections::HashMap;
use std::hash::Hash;

use cratestack_core::{BatchResponse, CoolContext, CoolError};

use crate::query::support::push_action_policy_query;
use crate::{ModelDescriptor, ModelPrimaryKey, SqlxRuntime, sqlx};

use super::validate::{reject_duplicate_pks, validate_batch_size};

#[derive(Debug, Clone)]
pub struct BatchGet<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchGet<'a, M, PK> {
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + ModelPrimaryKey<PK>,
        PK: Clone
            + Eq
            + Hash
            + Send
            + sqlx::Type<sqlx::Postgres>
            + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        validate_batch_size(self.ids.len())?;
        reject_duplicate_pks(&self.ids)?;
        if self.ids.is_empty() {
            return Ok(BatchResponse::from_results(vec![]));
        }

        // Single SELECT with IN-list + read policy + soft-delete filter.
        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("SELECT ");
        query.push(self.descriptor.select_projection());
        query.push(" FROM ").push(self.descriptor.table_name);
        query.push(" WHERE ");
        if let Some(col) = self.descriptor.soft_delete_column {
            query.push(col).push(" IS NULL AND ");
        }
        query.push(self.descriptor.primary_key).push(" IN (");
        for (index, id) in self.ids.iter().enumerate() {
            if index > 0 {
                query.push(", ");
            }
            query.push_bind(id.clone());
        }
        query.push(") AND ");
        push_action_policy_query(
            &mut query,
            self.descriptor.read_allow_policies,
            self.descriptor.read_deny_policies,
            ctx,
        );

        let rows: Vec<M> = query
            .build_query_as::<M>()
            .fetch_all(self.runtime.pool())
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        // Walk-and-match: pair each input PK back to its row, or
        // NotFound when the read policy / soft-delete excluded it.
        let mut by_pk: HashMap<PK, M> = rows.into_iter().map(|m| (m.primary_key(), m)).collect();
        let per_item: Vec<Result<M, CoolError>> = self
            .ids
            .into_iter()
            .map(|id| {
                by_pk
                    .remove(&id)
                    .ok_or_else(|| CoolError::NotFound("no row matched".to_owned()))
            })
            .collect();

        Ok(BatchResponse::from_results(per_item))
    }
}
