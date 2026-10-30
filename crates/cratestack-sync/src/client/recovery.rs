//! Turning a server error into local state the next step can work from.

use cratestack_rusqlite::rusqlite::Connection;

use super::flow::Recovery;
use super::{ClientError, SyncEngine, store};
use crate::protocol::{Cursor, RejectCode, SyncErrorBody, SyncErrorCode};

impl SyncEngine {
    /// Prepare local state for whatever the server's error requires.
    pub fn recover(
        &self,
        conn: &mut Connection,
        error: &SyncErrorBody,
    ) -> Result<Recovery, ClientError> {
        let tx = conn.transaction()?;
        let recovery = match error.code {
            SyncErrorCode::ClientExpired => {
                self.reset_registration(&tx)?;
                Recovery::Reregister
            }
            SyncErrorCode::CursorExpired => {
                store::set(&tx, store::CURSOR, Cursor::ZERO)?;
                store::set(&tx, store::RESYNC, 1)?;
                Recovery::Resync
            }
            SyncErrorCode::SequenceGap => {
                let server_seq = error.last_mutation_seq.unwrap_or(0);
                store::set(&tx, store::PUSHED_SEQ, server_seq)?;
                Recovery::Retry { after_secs: None }
            }
            SyncErrorCode::ClientMismatch
            | SyncErrorCode::Unauthenticated
            | SyncErrorCode::Forbidden => Recovery::Halt,
            _ => Recovery::Retry {
                after_secs: error.retry_after_secs,
            },
        };
        tx.commit()?;
        Ok(recovery)
    }

    /// The server forgot this client (client TTL). Mutations it consumed but
    /// never acknowledged have an unknown fate, so they are surfaced as
    /// rejected rather than re-sent under a new id and possibly applied
    /// twice. The rest are renumbered from 1 for the new registration.
    fn reset_registration(&self, conn: &Connection) -> Result<(), ClientError> {
        let pushed = store::get_u64(conn, store::PUSHED_SEQ)?;
        for mutation in store::pending(conn, 0)?
            .into_iter()
            .filter(|m| m.seq <= pushed)
        {
            store::reject(
                conn,
                mutation.seq,
                RejectCode::Expired,
                "sync registration expired before the server acknowledged this change",
            )?;
        }
        // Two steps so no intermediate value collides with the primary key.
        conn.execute(
            "UPDATE cratestack_sync_pending SET seq = -(seq - ?1)",
            [pushed as i64],
        )?;
        conn.execute("UPDATE cratestack_sync_pending SET seq = -seq", [])?;
        let remaining: i64 =
            conn.query_row("SELECT count(*) FROM cratestack_sync_pending", [], |r| {
                r.get(0)
            })?;
        store::set(conn, store::NEXT_SEQ, remaining + 1)?;
        for key in [store::CLIENT_ID, store::PUSHED_SEQ, store::ACK_SEQ] {
            store::clear(conn, key)?;
        }
        store::set(conn, store::CURSOR, Cursor::ZERO)?;
        store::set(conn, store::RESYNC, 1)?;
        self.rewind(conn)?;
        self.replay(conn)?;
        Ok(())
    }
}
