//! Rewind and replay: the invariant the client maintains is
//!
//! ```text
//! local table state == server state as of the cursor  +  replay(pending, in seq order)
//! ```
//!
//! Every operation that changes either side (a pull, a rejection, a reset)
//! rewinds to the server state through the undo log, changes it, then
//! replays what is still pending. Replay re-captures pre-images, so the
//! undo log is always relative to the latest server state.

use cratestack_rusqlite::rusqlite::Connection;

use super::{ClientError, SyncEngine, ddl, store};

/// Run `f` with capture triggers on. Always switches them off again.
pub(crate) fn capturing<T>(
    conn: &Connection,
    f: impl FnOnce() -> Result<T, ClientError>,
) -> Result<T, ClientError> {
    conn.execute(ddl::set_capture(true), [])?;
    let result = f();
    conn.execute(ddl::set_capture(false), [])?;
    result
}

impl SyncEngine {
    /// Restore every row touched by a pending mutation to server state.
    pub(crate) fn rewind(&self, conn: &Connection) -> Result<(), ClientError> {
        for model in &self.models {
            conn.execute_batch(&ddl::rewind_sql(
                model.model(),
                model.table(),
                model.pk_column(),
            ))?;
        }
        conn.execute("DELETE FROM cratestack_sync_dirty", [])?;
        Ok(())
    }

    /// Re-apply every pending mutation in seq order. A mutation that no
    /// longer applies locally (its row was deleted on the server, say) is
    /// rolled back and left pending: the server, not the client, decides
    /// whether it is still valid.
    pub(crate) fn replay(&self, conn: &Connection) -> Result<(), ClientError> {
        for mutation in store::pending(conn, 0)? {
            let Some(mutator) = self.mutators.get(&mutation.name) else {
                continue;
            };
            conn.execute_batch("SAVEPOINT cratestack_sync_replay")?;
            match capturing(conn, || mutator.apply(conn, &mutation.args)) {
                Ok(()) => conn.execute_batch("RELEASE cratestack_sync_replay")?,
                Err(_) => conn.execute_batch(
                    "ROLLBACK TO cratestack_sync_replay; RELEASE cratestack_sync_replay",
                )?,
            }
        }
        Ok(())
    }

    /// Drop every synced row (before a full resync from cursor zero).
    pub(crate) fn wipe(&self, conn: &Connection) -> Result<(), ClientError> {
        for model in &self.models {
            conn.execute_batch(&format!("DELETE FROM {}", model.table()))?;
        }
        Ok(())
    }
}
