//! `payload_before` on an outbox table an earlier version created.
//!
//! The column is part of `ensure_event_outbox_table`'s DDL, but `CREATE TABLE
//! IF NOT EXISTS` leaves an old table as it is. Adding the column cannot sit
//! in that per-write statement (it runs in every write transaction of every
//! `@@emit` model, and `ALTER TABLE` holds `ACCESS EXCLUSIVE` until its
//! transaction ends), so it is done here instead:
//!
//! - in its own short transaction, at the points that own their connection:
//!   the start of an update or upsert of an `@@subscribe` model that opens its
//!   own transaction. Never inside a caller's transaction: that needs a second
//!   pool connection while the caller holds one, which deadlocks a small pool;
//! - an `ALTER TABLE` waiting for `ACCESS EXCLUSIVE` makes every later writer
//!   of the table queue behind it, so it gives up after 100 ms, runs one at a
//!   time per runtime, and after a failure is not retried for a minute;
//! - the table is looked up with `to_regclass`, which resolves through the
//!   `search_path` exactly as the outbox `INSERT` does;
//! - `ADD COLUMN IF NOT EXISTS`, so two sessions upgrading at once both succeed.
//!
//! Nothing else needs the column. Reading the outbox is tolerant of its
//! absence ([`super::event_outbox`]), and a write inside a caller's
//! transaction ([`SqlxRuntime::keeps_before`]) only reads the catalog and
//! stores no pre-image while the column is missing: its update sends no
//! filter-exit event, and nothing fails.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cratestack_core::CratestackError;

use super::SqlxRuntime;
use crate::error::cratestack_error_from_sqlx;
use crate::{ModelDescriptor, sqlx};

/// After a failed (or empty-handed) attempt, the next one waits this long.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// What a runtime knows of the outbox table's `payload_before` column.
#[derive(Debug, Default)]
pub(crate) struct OutboxColumn {
    present: AtomicBool,
    attempting: AtomicBool,
    last_attempt: Mutex<Option<Instant>>,
}

/// Holds the right to attempt an upgrade; releases it on drop.
struct Attempt<'a>(&'a AtomicBool);

impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl OutboxColumn {
    fn is_present(&self) -> bool {
        self.present.load(Ordering::Acquire)
    }

    fn set_present(&self) {
        self.present.store(true, Ordering::Release);
    }

    /// Called when an `INSERT` naming the column failed: whatever made the
    /// flag true (a read in a transaction that has since rolled back) is
    /// stale, so the next write checks again.
    pub(crate) fn forget(&self) {
        self.present.store(false, Ordering::Release);
    }

    /// The right to attempt an upgrade now: nobody else is, and the last
    /// attempt was long enough ago.
    fn attempt(&self) -> Option<Attempt<'_>> {
        if self.attempting.swap(true, Ordering::AcqRel) {
            return None;
        }
        let attempt = Attempt(&self.attempting);
        let mut last = self.last_attempt.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return None;
        }
        *last = Some(Instant::now());
        Some(attempt)
    }
}

/// Whether the outbox table on this connection's `search_path` has
/// `payload_before`; `None` when there is no such table yet.
async fn has_before_column<'e, E>(executor: E) -> Result<Option<bool>, CratestackError>
where
    E: sqlx::Executor<'e, Database = sqlx::Postgres>,
{
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_attribute \
            WHERE attrelid = t.rel AND attname = 'payload_before' AND NOT attisdropped) \
         FROM (SELECT to_regclass('cratestack_event_outbox') AS rel) t WHERE t.rel IS NOT NULL",
    )
    .fetch_optional(executor)
    .await
    .map_err(cratestack_error_from_sqlx)
}

/// Adds the column to an old table, if there is one. Returns whether the
/// table now has it.
pub(super) async fn upgrade_outbox_table(pool: &sqlx::PgPool) -> Result<bool, CratestackError> {
    match has_before_column(pool).await? {
        Some(false) => {}
        present => return Ok(present == Some(true)),
    }
    let mut tx = pool.begin().await.map_err(cratestack_error_from_sqlx)?;
    for statement in [
        "SET LOCAL lock_timeout = '100ms'",
        "ALTER TABLE cratestack_event_outbox ADD COLUMN IF NOT EXISTS payload_before JSONB",
    ] {
        sqlx::query(statement)
            .execute(&mut *tx)
            .await
            .map_err(cratestack_error_from_sqlx)?;
    }
    tx.commit().await.map_err(cratestack_error_from_sqlx)?;
    Ok(true)
}

impl SqlxRuntime {
    /// Upgrade the outbox table before an update or upsert of this model
    /// (one that keeps a pre-image) opens its transaction; see the module
    /// docs for when it runs. A failure is logged.
    pub(crate) async fn upgrade_event_outbox<M, PK>(
        &self,
        descriptor: &ModelDescriptor<M, PK>,
        emits_updated: bool,
    ) {
        // A bound runtime's one connection is the procedure's transaction.
        if self.bound.is_some()
            || !(emits_updated && descriptor.event_target().notify_subscribers)
            || self.outbox_column.is_present()
        {
            return;
        }
        let Some(_attempt) = self.outbox_column.attempt() else {
            return;
        };
        match upgrade_outbox_table(&self.pool).await {
            Ok(true) => self.outbox_column.set_present(),
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(target: "cratestack", %error, "event outbox not upgraded");
            }
        }
    }

    /// The outbox column, when an update of this model should keep its
    /// pre-image on the event: the model is `@@subscribe`, emits `updated`,
    /// and the outbox table can store it (checked on `executor` until it is
    /// seen to).
    pub(crate) async fn keeps_before<'e, E, M, PK>(
        &self,
        executor: E,
        descriptor: &ModelDescriptor<M, PK>,
        emits_updated: bool,
    ) -> Result<Option<&OutboxColumn>, CratestackError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if !(emits_updated && descriptor.event_target().notify_subscribers) {
            return Ok(None);
        }
        let column = &*self.outbox_column;
        if !column.is_present() && has_before_column(executor).await? == Some(true) {
            column.set_present();
        }
        Ok(column.is_present().then_some(column))
    }
}

#[cfg(test)]
mod tests;
