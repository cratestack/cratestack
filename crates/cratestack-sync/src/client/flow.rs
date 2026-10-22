//! The request/response steps the host drives. Error recovery is in
//! `recovery.rs`.

use cratestack_rusqlite::rusqlite::Connection;
use serde::Serialize;

use super::{ClientError, SyncEngine, store};
use crate::protocol::{
    ACK_MODEL, ChangeOp, MutationOutcome, PullRequest, PullResponse, PushRequest, PushResponse,
    RegisterRequest, RegisterResponse,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushOutcome {
    pub applied: usize,
    pub rejected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullOutcome {
    pub changes: usize,
    pub acked: u64,
    pub has_more: bool,
}

/// What the host should do after a request failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Recovery {
    /// Register again, then sync. Local state was prepared for it.
    Reregister,
    /// Pull again from zero. Local state will be replaced on arrival.
    Resync,
    /// Try the same step again, after `after_secs` if given.
    Retry { after_secs: Option<u32> },
    /// Stop syncing: the credentials do not match this database's owner.
    /// The host must sign in again or discard local data — the engine will
    /// not push one principal's queued writes as another.
    Halt,
}

impl SyncEngine {
    pub fn register_request(&self, label: Option<String>) -> RegisterRequest {
        RegisterRequest { label }
    }

    pub fn apply_register(
        &self,
        conn: &mut Connection,
        response: RegisterResponse,
    ) -> Result<(), ClientError> {
        let tx = conn.transaction()?;
        store::set(&tx, store::CLIENT_ID, response.client_id)?;
        store::set(&tx, store::MUTATION_TTL, response.mutation_ttl_secs)?;
        store::set(&tx, store::PUSHED_SEQ, 0)?;
        store::set(&tx, store::ACK_SEQ, 0)?;
        tx.commit()?;
        Ok(())
    }

    /// The next batch of unsent mutations, or `None` if there is nothing to
    /// send or the client is not registered yet.
    pub fn push_request(
        &self,
        conn: &Connection,
        max: usize,
    ) -> Result<Option<PushRequest>, ClientError> {
        let Some(client_id) = store::client_id(conn)? else {
            return Ok(None);
        };
        let pushed = store::get_u64(conn, store::PUSHED_SEQ)?;
        let mut mutations = store::pending(conn, pushed)?;
        mutations.truncate(max);
        Ok((!mutations.is_empty()).then_some(PushRequest {
            client_id,
            mutations,
        }))
    }

    pub fn apply_push(
        &self,
        conn: &mut Connection,
        response: PushResponse,
    ) -> Result<PushOutcome, ClientError> {
        let tx = conn.transaction()?;
        let mut outcome = PushOutcome {
            applied: 0,
            rejected: 0,
        };
        for result in &response.results {
            match &result.outcome {
                MutationOutcome::Rejected { code, message } => {
                    store::reject(&tx, result.seq, *code, message)?;
                    outcome.rejected += 1;
                }
                _ => outcome.applied += 1,
            }
        }
        let pushed = store::get_u64(&tx, store::PUSHED_SEQ)?;
        store::set(
            &tx,
            store::PUSHED_SEQ,
            pushed.max(response.last_mutation_seq),
        )?;
        if outcome.rejected > 0 {
            // Undo the rejected mutations' optimistic effects.
            self.rewind(&tx)?;
            self.replay(&tx)?;
        }
        tx.commit()?;
        Ok(outcome)
    }

    pub fn pull_request(&self, conn: &Connection) -> Result<Option<PullRequest>, ClientError> {
        let Some(client_id) = store::client_id(conn)? else {
            return Ok(None);
        };
        Ok(Some(PullRequest {
            client_id,
            cursor: store::cursor(conn)?,
            limit: None,
        }))
    }

    pub fn apply_pull(
        &self,
        conn: &mut Connection,
        response: PullResponse,
    ) -> Result<PullOutcome, ClientError> {
        let client_id = store::client_id(conn)?.map(|id| id.to_string());
        let tx = conn.transaction()?;
        self.rewind(&tx)?;
        if store::get(&tx, store::RESYNC)?.is_some() {
            self.wipe(&tx)?;
            store::clear(&tx, store::RESYNC)?;
        }
        let mut ack = store::get_u64(&tx, store::ACK_SEQ)?;
        for change in &response.changes {
            if change.model == ACK_MODEL {
                if let (ChangeOp::Upsert { row }, Some(own)) = (&change.op, &client_id)
                    && &change.pk == own
                {
                    ack = ack.max(row.get("seq").and_then(|s| s.as_u64()).unwrap_or(0));
                }
                continue;
            }
            // A model this build does not know is skipped, not fatal: an
            // older app version keeps syncing the models it has.
            let Some(model) = self.find_model(&change.model) else {
                continue;
            };
            match &change.op {
                ChangeOp::Upsert { row } => model.upsert(&tx, row)?,
                ChangeOp::Delete => model.delete(&tx, &change.pk)?,
            }
        }
        tx.execute(
            "DELETE FROM cratestack_sync_pending WHERE seq <= ?1",
            [ack as i64],
        )?;
        store::set(&tx, store::ACK_SEQ, ack)?;
        store::set(&tx, store::CURSOR, response.cursor)?;
        self.replay(&tx)?;
        tx.commit()?;
        Ok(PullOutcome {
            changes: response.changes.len(),
            acked: ack,
            has_more: response.has_more,
        })
    }
}
