//! A minimal in-memory [`IdempotencyStore`] for tests whose subject is
//! *admission* — whether a reservation is attempted at all — rather than
//! how a reservation is persisted.
//!
//! The persistent story is already covered against real Postgres by
//! `tests/banking_idempotency.rs` and against real Redis by
//! `cratestack-redis/tests/e2e.rs`; duplicating it here would slow the
//! suite down and add a second thing to keep in sync. Behaviourally this
//! is the same shape as the fake
//! `cratestack-api/tests/default_fingerprint_collision.rs` already uses.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use cratestack::CratestackError;
use cratestack_axum::idempotency::{IdempotencyRecord, IdempotencyStore, ReservationOutcome};

struct Entry {
    token: uuid::Uuid,
    hash: [u8; 32],
    record: Option<IdempotencyRecord>,
}

#[derive(Default)]
pub struct InMemoryIdempotencyStore {
    entries: Mutex<HashMap<(String, String), Entry>>,
    /// Counts `reserve_or_fetch` calls, so "this op took no reservation"
    /// can be asserted directly instead of inferred from the absence of a
    /// conflict response.
    ///
    /// Private, and named differently from its accessor: a `pub` field and
    /// a `pub fn` sharing one name compile, but then `store.reserve_calls`
    /// and `store.reserve_calls()` are different types at every call site,
    /// which is a footgun in a test helper whose whole job is being read
    /// at a glance.
    reserve_count: AtomicUsize,
}

impl InMemoryIdempotencyStore {
    pub fn reserve_calls(&self) -> usize {
        self.reserve_count.load(Ordering::SeqCst)
    }
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
        self.reserve_count.fetch_add(1, Ordering::SeqCst);
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
            entry.record = Some(IdempotencyRecord {
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
