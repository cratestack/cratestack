//! Read-only views of sync state for a UI.

use cratestack_rusqlite::rusqlite::Connection;

use super::{ClientError, SyncEngine, SyncStatus, store};

impl SyncEngine {
    pub fn status(&self, conn: &Connection) -> Result<SyncStatus, ClientError> {
        let pushed = store::get_u64(conn, store::PUSHED_SEQ)?;
        let pending: i64 =
            conn.query_row("SELECT count(*) FROM cratestack_sync_pending", [], |r| {
                r.get(0)
            })?;
        let unpushed: i64 = conn.query_row(
            "SELECT count(*) FROM cratestack_sync_pending WHERE seq > ?1",
            [pushed as i64],
            |r| r.get(0),
        )?;
        Ok(SyncStatus {
            client_id: store::client_id(conn)?,
            cursor: store::cursor(conn)?,
            pending: pending as u64,
            unpushed: unpushed as u64,
            rejected: store::rejected(conn)?,
        })
    }

    /// Primary keys of `model` rows that carry an unacknowledged local
    /// change — what a UI marks as "not synced yet". Read from the undo
    /// log, so it is exact: a key leaves this list when the server state
    /// that includes its change arrives.
    pub fn unsynced_keys(
        &self,
        conn: &Connection,
        model: &str,
    ) -> Result<Vec<String>, ClientError> {
        let mut stmt =
            conn.prepare("SELECT CAST(pk AS TEXT) FROM cratestack_sync_dirty WHERE model = ?1")?;
        let keys = stmt.query_map([model], |row| row.get(0))?;
        Ok(keys.collect::<Result<_, _>>()?)
    }

    /// Forget rejected mutations once the user has seen them.
    pub fn clear_rejected(&self, conn: &Connection) -> Result<(), ClientError> {
        conn.execute("DELETE FROM cratestack_sync_rejected", [])?;
        Ok(())
    }
}
