//! Single-row UPDATE with optional version locking, policy, audit + events.

use cratestack_core::{AuditOperation, CratestackContext, CratestackError, ModelEventKind};

use crate::audit::{
    RunInTxOutcome, build_audit_event, enqueue_audit_event, ensure_audit_table, fetch_for_audit,
};
use crate::descriptor::{enqueue_event_outbox, ensure_event_outbox_table};
use crate::{ModelDescriptor, SqlxRuntime, UpdateModelInput, sqlx};

use super::preview::render_update_preview_sql;
use super::update_exec::update_record_with_executor;

#[derive(Debug, Clone)]
pub struct UpdateRecord<'a, M: 'static, PK: 'static> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
}

impl<'a, M: 'static, PK: 'static> UpdateRecord<'a, M, PK> {
    pub fn set<I>(self, input: I) -> UpdateRecordSet<'a, M, PK, I> {
        UpdateRecordSet {
            runtime: self.runtime,
            descriptor: self.descriptor,
            id: self.id,
            input,
            if_match: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateRecordSet<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) id: PK,
    pub(crate) input: I,
    pub(crate) if_match: Option<i64>,
}

impl<'a, M: 'static, PK: 'static, I> UpdateRecordSet<'a, M, PK, I>
where
    I: UpdateModelInput<M>,
{
    /// Expected version for optimistic locking. Required on models
    /// that declare `@version`; ignored otherwise.
    pub fn if_match(mut self, expected: i64) -> Self {
        self.if_match = Some(expected);
        self
    }

    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let columns: Vec<&str> = values.iter().map(|v| v.column).collect();
        render_update_preview_sql(
            self.descriptor.table_name,
            self.descriptor.primary_key,
            self.descriptor.version_column,
            &columns,
            &self.descriptor.select_projection(),
        )
    }

    /// Participates in a caller-supplied transaction. Neither the
    /// `AuditSink` fan-out nor the event outbox drain happens here —
    /// see `create.rs`'s `run_in_tx` doc comment for the full contract
    /// and how a caller opts into both after their own commit.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CratestackContext,
    ) -> Result<RunInTxOutcome<M>, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        if self.descriptor.version_column.is_some() && self.if_match.is_none() {
            return Err(CratestackError::PreconditionFailed(
                "If-Match header required for versioned model".to_owned(),
            ));
        }
        let emits_event = self.descriptor.emits(ModelEventKind::Updated);
        let audit_enabled = self.descriptor.audit_enabled;
        if emits_event {
            ensure_event_outbox_table(&mut **tx).await?;
        }
        if audit_enabled {
            ensure_audit_table(self.runtime).await?;
        }
        let before_record = if audit_enabled {
            fetch_for_audit(&mut **tx, self.descriptor, self.id.clone()).await?
        } else {
            None
        };
        let before_snapshot = before_record
            .as_ref()
            .and_then(|m| serde_json::to_value(m).ok());
        let record = update_record_with_executor(
            &mut **tx,
            self.runtime.pool(),
            self.descriptor,
            self.id,
            self.input,
            ctx,
            self.if_match,
        )
        .await?;
        if emits_event {
            enqueue_event_outbox(
                &mut **tx,
                self.descriptor.schema_name,
                ModelEventKind::Updated,
                &record,
            )
            .await?;
        }
        let mut audit_event = None;
        if audit_enabled {
            let after = serde_json::to_value(&record).ok();
            let event = build_audit_event(
                self.descriptor,
                AuditOperation::Update,
                before_snapshot,
                after,
                ctx,
            );
            enqueue_audit_event(&mut **tx, &event).await?;
            audit_event = Some(event);
        }
        Ok(RunInTxOutcome::new(
            record,
            audit_event.into_iter().collect(),
        ))
    }

    pub async fn run(self, ctx: &CratestackContext) -> Result<M, CratestackError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + Clone + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        super::update_run::run_update(
            self.runtime,
            self.descriptor,
            self.id,
            self.input,
            self.if_match,
            ctx,
        )
        .await
    }
}
