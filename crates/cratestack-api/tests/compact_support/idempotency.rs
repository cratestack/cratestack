//! In-memory store for compact retry integration tests. The real HTTP layer
//! computes the hash and makes the reserve/replay/conflict decision.

use cratestack::CratestackError;
use cratestack_axum::idempotency::{IdempotencyStore, ReservationOutcome};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

#[derive(Default)]
pub struct InMemoryIdempotencyStore {
    entries: Mutex<HashMap<(String, String), Entry>>,
}

struct Entry {
    token: uuid::Uuid,
    hash: [u8; 32],
    record: Option<cratestack_axum::idempotency::IdempotencyRecord>,
}

#[async_trait::async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        match entries.get(&map_key) {
            None => {
                let token = uuid::Uuid::new_v4();
                entries.insert(
                    map_key,
                    Entry {
                        token,
                        hash: request_hash,
                        record: None,
                    },
                );
                Ok(ReservationOutcome::Reserved { token })
            }
            Some(entry) if entry.hash != request_hash => Ok(ReservationOutcome::Conflict),
            Some(entry) => Ok(match &entry.record {
                Some(record) => ReservationOutcome::Replay(record.clone()),
                None => ReservationOutcome::InFlight,
            }),
        }
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(&(principal.to_owned(), key.to_owned()))
            && entry.token == token
        {
            entry.record = Some(cratestack_axum::idempotency::IdempotencyRecord {
                key: key.to_owned(),
                principal_fingerprint: principal.to_owned(),
                request_hash: entry.hash,
                response_status: status,
                response_headers: headers.to_vec(),
                response_body: body.to_vec(),
                created_at: SystemTime::now(),
                expires_at: SystemTime::now(),
            });
        }
        Ok(())
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        let mut entries = self.entries.lock().unwrap();
        let map_key = (principal.to_owned(), key.to_owned());
        if entries
            .get(&map_key)
            .is_some_and(|entry| entry.token == token)
        {
            entries.remove(&map_key);
        }
        Ok(())
    }
}
