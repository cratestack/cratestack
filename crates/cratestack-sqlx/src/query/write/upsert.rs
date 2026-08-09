//! `INSERT … ON CONFLICT (<pk>) DO UPDATE …`, but with the
//! create/update distinction made *before* the SQL runs (via a
//! `SELECT … FOR UPDATE` probe inside the same transaction) so we can:
//!
//!   * pick the right policy slot (both must allow at call time)
//!   * emit the correct ModelEventKind (Created vs Updated)
//!   * capture an audit `before` snapshot only on the update branch
//!
//! The upsert is always transactional regardless of whether the model
//! emits events or has `@@audit`. One extra round-trip for the
//! SELECT, in exchange for clean event/audit semantics. Upsert is not
//! a hot read path — callers who need raw insert/update throughput
//! should use `.create()` / `.update()` directly.

use cratestack_core::{CoolContext, CoolError};

use crate::{
    ConflictTarget, ModelDescriptor, SqlxRuntime, UpsertModelInput, cool_error_from_sqlx, sqlx,
};

use super::upsert_do_nothing::UpsertRecordDoNothing;
use super::upsert_exec::run_upsert_in_tx;

#[derive(Debug, Clone)]
pub struct UpsertRecord<'a, M: 'static, PK: 'static, I> {
    pub(crate) runtime: &'a SqlxRuntime,
    pub(crate) descriptor: &'static ModelDescriptor<M, PK>,
    pub(crate) input: I,
    pub(crate) conflict_target: ConflictTarget,
}

impl<'a, M: 'static, PK: 'static, I> UpsertRecord<'a, M, PK, I>
where
    I: UpsertModelInput<M>,
{
    /// Choose the conflict target. Defaults to the model's primary
    /// key; pass [`ConflictTarget::Columns`] to upsert on a composite
    /// unique key instead. The named columns must form a `UNIQUE`
    /// constraint/index on the target table.
    pub fn on_conflict(mut self, target: ConflictTarget) -> Self {
        self.conflict_target = target;
        self
    }

    /// Switch this call to `ON CONFLICT ... DO NOTHING` semantics
    /// (cratestack#487), independent of `descriptor.upsert_update_columns`:
    /// on conflict, leave the existing row completely untouched instead
    /// of merging `upsert_update_columns` into it. This is the
    /// idempotent-insert shape ledger-style writes need — e.g. a
    /// cash-in claim that inserts a `PENDING` row and treats a
    /// conflict as "already in flight" must never let a retry's blank
    /// values overwrite an existing `COMPLETED` row's `transfer_ref`.
    ///
    /// Returns a distinct builder type rather than a flag on
    /// `UpsertRecord` because the return shape genuinely changes: a
    /// real `DO NOTHING` returns nothing on conflict, so the caller
    /// needs `Inserted` vs `Existing` distinguishable in the type
    /// ([`crate::UpsertOutcome`]) rather than collapsed into a plain
    /// `M` the way `.run()` returns it today. Encoding that as a
    /// separate type also means existing `.upsert(..).run(..)` callers
    /// keep their `Result<M, CoolError>` signature unchanged — this is
    /// purely additive, not a behavior change for the DO UPDATE path.
    pub fn do_nothing(self) -> UpsertRecordDoNothing<'a, M, PK, I> {
        UpsertRecordDoNothing {
            runtime: self.runtime,
            descriptor: self.descriptor,
            input: self.input,
            conflict_target: self.conflict_target,
        }
    }

    /// Render an approximate SQL preview. The actual upsert wraps a
    /// `SELECT … FOR UPDATE` around the `INSERT … ON CONFLICT`, but
    /// this preview returns only the conflict-bearing statement.
    pub fn preview_sql(&self) -> String {
        let values = self.input.sql_values();
        let placeholders = (1..=values.len())
            .map(|index| format!("${index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let columns = values
            .iter()
            .map(|value| value.column)
            .collect::<Vec<_>>()
            .join(", ");
        let update_assignments = self
            .descriptor
            .upsert_update_columns
            .iter()
            .map(|column| format!("{column} = EXCLUDED.{column}"))
            .collect::<Vec<_>>()
            .join(", ");
        let version_bump = match self.descriptor.version_column {
            Some(col) => format!(
                ", {col} = {table}.{col} + 1",
                table = self.descriptor.table_name,
                col = col
            ),
            None => String::new(),
        };
        let conflict_tuple = match self.conflict_target {
            ConflictTarget::PrimaryKey => self.descriptor.primary_key.to_owned(),
            ConflictTarget::Columns(cols) => cols.join(", "),
        };

        format!(
            "INSERT INTO {table} ({columns}) VALUES ({placeholders}) \
             ON CONFLICT ({conflict_tuple}) DO UPDATE SET {update_assignments}{version_bump} \
             RETURNING {projection}",
            table = self.descriptor.table_name,
            projection = self.descriptor.select_projection(),
        )
    }

    pub async fn run(self, ctx: &CoolContext) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let runtime = self.runtime;
        let mut tx = runtime.pool().begin().await.map_err(cool_error_from_sqlx)?;
        let (record, emits_event) = run_upsert_in_tx(
            &mut tx,
            runtime,
            self.descriptor,
            self.input,
            self.conflict_target,
            ctx,
        )
        .await?;
        tx.commit().await.map_err(cool_error_from_sqlx)?;
        if emits_event {
            let _ = runtime.drain_event_outbox().await;
        }
        Ok(record)
    }

    /// Like [`Self::run`] but participates in a caller-supplied
    /// transaction. The conflict probe runs against `tx`, so the row
    /// lock is held until the caller commits. The event outbox is not
    /// drained here.
    pub async fn run_in_tx<'tx>(
        self,
        tx: &mut sqlx::Transaction<'tx, sqlx::Postgres>,
        ctx: &CoolContext,
    ) -> Result<M, CoolError>
    where
        for<'r> M: Send + Unpin + sqlx::FromRow<'r, sqlx::postgres::PgRow> + serde::Serialize,
        PK: Send + sqlx::Type<sqlx::Postgres> + for<'q> sqlx::Encode<'q, sqlx::Postgres>,
    {
        let (record, _) = run_upsert_in_tx(
            tx,
            self.runtime,
            self.descriptor,
            self.input,
            self.conflict_target,
            ctx,
        )
        .await?;
        Ok(record)
    }
}
