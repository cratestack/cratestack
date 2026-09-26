//! An in-memory idempotency store that keeps the response headers (so a
//! replay is CBOR again, as with the real stores), and a rate-limit store
//! that records every bucket key it is charged.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::SystemTime;

use async_trait::async_trait;
use cratestack::{
    BoundedOutcome, ConsumeRequest, CratestackError, RateLimitConfig, RateLimitDecision,
    RateLimitStore,
};
use cratestack_axum::idempotency::{IdempotencyRecord, IdempotencyStore, ReservationOutcome};
use cratestack_axum::ratelimit::InMemoryRateLimitStore;

type Slot = (String, String);

#[derive(Default)]
pub struct MemoryIdempotency {
    rows: Mutex<HashMap<Slot, (uuid::Uuid, [u8; 32], Option<IdempotencyRecord>)>>,
}

#[async_trait]
impl IdempotencyStore for MemoryIdempotency {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        hash: [u8; 32],
        _expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CratestackError> {
        let mut rows = self.rows.lock().unwrap();
        let slot = (principal.to_owned(), key.to_owned());
        Ok(match rows.get(&slot) {
            None => {
                let token = uuid::Uuid::new_v4();
                rows.insert(slot, (token, hash, None));
                ReservationOutcome::Reserved { token }
            }
            Some((_, stored, _)) if *stored != hash => ReservationOutcome::Conflict,
            Some((_, _, Some(record))) => ReservationOutcome::Replay(record.clone()),
            Some((_, _, None)) => ReservationOutcome::InFlight,
        })
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
        let mut rows = self.rows.lock().unwrap();
        if let Some((stored, hash, record)) = rows.get_mut(&(principal.to_owned(), key.to_owned()))
            && *stored == token
        {
            *record = Some(IdempotencyRecord {
                key: key.to_owned(),
                principal_fingerprint: principal.to_owned(),
                request_hash: *hash,
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
        let mut rows = self.rows.lock().unwrap();
        let slot = (principal.to_owned(), key.to_owned());
        if rows
            .get(&slot)
            .is_some_and(|(stored, _, _)| *stored == token)
        {
            rows.remove(&slot);
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct SpyRateLimit {
    inner: InMemoryRateLimitStore,
    pub keys: Mutex<Vec<String>>,
}

#[async_trait]
impl RateLimitStore for SpyRateLimit {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.keys.lock().unwrap().push(key.to_owned());
        self.inner.consume(key, config).await
    }

    async fn consume_bounded(
        &self,
        request: ConsumeRequest<'_>,
    ) -> Result<BoundedOutcome, CratestackError> {
        self.keys.lock().unwrap().push(request.key.to_owned());
        self.inner.consume_bounded(request).await
    }
}
