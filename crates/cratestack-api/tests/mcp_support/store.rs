//! An in-memory `IdempotencyStore` that really records completions, so a
//! replay returns what the first call produced. `cratestack-axum` ships no
//! public in-memory idempotency store (only rate-limit), hence this.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use cratestack::{CratestackError, IdempotencyRecord, IdempotencyStore, ReservationOutcome};

type Slot = (String, String);

#[derive(Default)]
pub struct MemoryStore {
    rows: Mutex<HashMap<Slot, ([u8; 32], uuid::Uuid, Option<(u16, Vec<u8>)>)>>,
}

#[async_trait::async_trait]
impl IdempotencyStore for MemoryStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        let mut rows = self.rows.lock().unwrap();
        let slot = (principal.to_owned(), key.to_owned());
        if let Some((hash, _, done)) = rows.get(&slot) {
            if *hash != request_hash {
                return Ok(ReservationOutcome::Conflict);
            }
            return Ok(match done {
                None => ReservationOutcome::InFlight,
                Some((status, body)) => ReservationOutcome::Replay(IdempotencyRecord {
                    key: key.to_owned(),
                    principal_fingerprint: principal.to_owned(),
                    request_hash,
                    response_status: *status,
                    response_headers: Vec::new(),
                    response_body: body.clone(),
                    created_at: SystemTime::now(),
                    expires_at,
                }),
            });
        }
        let token = uuid::Uuid::new_v4();
        rows.insert(slot, (request_hash, token, None));
        Ok(ReservationOutcome::Reserved { token })
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        _headers: &[u8],
        body: &[u8],
    ) -> Result<(), CratestackError> {
        let mut rows = self.rows.lock().unwrap();
        if let Some(row) = rows.get_mut(&(principal.to_owned(), key.to_owned()))
            && row.1 == token
        {
            row.2 = Some((status, body.to_vec()));
        }
        Ok(())
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        _token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        self.rows
            .lock()
            .unwrap()
            .remove(&(principal.to_owned(), key.to_owned()));
        Ok(())
    }
}
