//! `batch_delete` — single `DELETE ... RETURNING` (or `UPDATE` for
//! soft-delete) with the delete policy in the WHERE. Per-item audit
//! and outbox events fan out from the RETURNING rows.

use std::collections::HashMap;
use std::hash::Hash;

use cratestack_core::{AuditOperation, BatchResponse, CoolContext, CoolError, ModelEventKind};

use crate::audit::{build_audit_event, enqueue_audit_event, ensure_audit_table};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::query::support::push_action_policy_query;
use crate::{ModelDescriptor, ModelPrimaryKey, SqlxRuntime, sqlx};

use super::validate::{reject_duplicate_pks, validate_batch_size};

#[derive(Debug, Clone)]
pub struct BatchDelete<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) ids: Vec<PK>,
}

impl<'a, M: 'static, PK: 'static> BatchDelete<'a, M, PK> {
    pub async fn run(self, ctx: &CoolContext) -> Result<BatchResponse<M>, CoolError>
    where
        for<'r> M: Send
            + Unpin
            + sqlx::FromRow<'r, sqlx::postgres::PgRow>
            + ModelPrimaryKey<PK>
            + serde::Serialize,
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

        let emits_event = self.descriptor.emits(ModelEventKind::Deleted);
        let audit_enabled = self.descriptor.audit_enabled;

        let mut tx = self
            .runtime
            .pool()
            .begin()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        if emits_event {
            ensure_event_outbox_table(&mut *tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }

        let mut query = sqlx::QueryBuilder::<sqlx::Postgres>::new("");
        match self.descriptor.soft_delete_column {
            Some(col) => {
                query.push("UPDATE ").push(self.descriptor.table_name);
                query.push(" SET ").push(col).push(" = NOW()");
                if let Some(version_col) = self.descriptor.version_column {
                    query
                        .push(", ")
                        .push(version_col)
                        .push(" = ")
                        .push(version_col)
                        .push(" + 1");
                }
                query.push(" WHERE ").push(col).push(" IS NULL AND ");
            }
            None => {
                query.push("DELETE FROM ").push(self.descriptor.table_name);
                query.push(" WHERE ");
            }
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
            self.descriptor.delete_allow_policies,
            self.descriptor.delete_deny_policies,
            ctx,
        );
        query
            .push(" RETURNING ")
            .push(self.descriptor.select_projection());

        let deleted: Vec<M> = query
            .build_query_as::<M>()
            .fetch_all(&mut *tx)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        // The RETURNING row IS the "before" snapshot — DELETE/soft-
        // delete returns the pre-mutation state.
        for record in &deleted {
            if emits_event {
                enqueue_event_outbox(
                    &mut *tx,
                    self.descriptor.schema_name,
                    ModelEventKind::Deleted,
                    record,
                )
                .await?;
            }
            if audit_enabled {
                let before = serde_json::to_value(record).ok();
                let event =
                    build_audit_event(self.descriptor, AuditOperation::Delete, before, None, ctx);
                enqueue_audit_event(&mut *tx, &event).await?;
            }
        }

        tx.commit()
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;

        if emits_event {
            let _ = self.runtime.drain_event_outbox().await;
        }

        // Walk-and-match: any input id whose row isn't in `deleted`
        // failed the WHERE (tombstoned, policy denied, never existed).
        // All three collapse to NotFound on the wire.
        let mut by_pk: HashMap<PK, M> = deleted.into_iter().map(|m| (m.primary_key(), m)).collect();
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
