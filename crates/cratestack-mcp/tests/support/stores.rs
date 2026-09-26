//! Minimal in-memory stores for the two admission concerns. Real enough to
//! exercise replay (the idempotency store keeps the completed body) and
//! throttling (the rate-limit store counts), nothing more.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::idempotency_record::{IdempotencyRecord, ReservationOutcome};
use cratestack_core::{
    CratestackError, IdempotencyStore, RateLimitConfig, RateLimitDecision, RateLimitStore,
};
use sha2::Digest;

#[derive(Default)]
pub struct MemoryIdempotency {
    rows: Mutex<HashMap<(String, String), Row>>,
}

struct Row {
    hash: [u8; 32],
    token: uuid::Uuid,
    done: Option<(u16, Vec<u8>)>,
}

#[async_trait]
impl IdempotencyStore for MemoryIdempotency {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        let mut rows = self.rows.lock().unwrap();
        let slot = (principal.to_owned(), key.to_owned());
        if let Some(row) = rows.get(&slot) {
            return Ok(match &row.done {
                _ if row.hash != request_hash => ReservationOutcome::Conflict,
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
        let row = Row {
            hash: request_hash,
            token,
            done: None,
        };
        rows.insert(slot, row);
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
            && row.token == token
        {
            row.done = Some((status, body.to_vec()));
        }
        Ok(())
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        _token: uuid::Uuid,
    ) -> Result<(), CratestackError> {
        let mut rows = self.rows.lock().unwrap();
        rows.remove(&(principal.to_owned(), key.to_owned()));
        Ok(())
    }
}

/// Allows the first `budget` calls per key, throttles the rest.
pub struct CountingLimiter {
    pub budget: u32,
    pub charged: AtomicU32,
    pub keys: Mutex<Vec<String>>,
}

impl CountingLimiter {
    pub fn new(budget: u32) -> Self {
        Self {
            budget,
            charged: AtomicU32::new(0),
            keys: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RateLimitStore for CountingLimiter {
    async fn consume(
        &self,
        key: &str,
        _config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.keys.lock().unwrap().push(key.to_owned());
        let charged = self.charged.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(if charged <= self.budget {
            RateLimitDecision::Allowed {
                remaining: self.budget - charged,
            }
        } else {
            RateLimitDecision::Throttled {
                retry_after_secs: 1,
            }
        })
    }
}

/// The rate-limit bucket (and idempotency namespace) MCP derives for an
/// `id` claim: `<prefix>:<sha256 hex of id>`, `prefix` being `mcp` for a
/// user and `mcp-system` for a `SystemContext` (cratestack#1033). Computed
/// here, not through the crate, so a change to what is hashed fails the
/// tests that compare against it.
pub fn bucket(prefix: &str, id: &str) -> String {
    let hex: String = sha2::Sha256::digest(id.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{prefix}:{hex}")
}
