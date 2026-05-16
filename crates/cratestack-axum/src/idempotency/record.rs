//! Persisted record + reservation-outcome state machine.

use std::time::SystemTime;

/// Persisted idempotency record returned on a replay. Banks need an
/// invariant view of the captured response — the store rebuilds this from
/// its persisted columns when the second caller asks to replay.
///
/// `response_headers` is an opaque blob produced by [`super::encode_headers`]
/// at capture time and consumed by [`super::decode_headers`] on replay. The
/// blob carries every end-to-end header the handler returned, including
/// `Location`, `ETag`, cache directives, and `Content-Type` — replaying
/// only the status + body would silently drop these and give a retry
/// different observable behaviour from the original execution.
#[derive(Debug, Clone)]
pub struct IdempotencyRecord {
    pub key: String,
    pub principal_fingerprint: String,
    pub request_hash: [u8; 32],
    pub response_status: u16,
    pub response_headers: Vec<u8>,
    pub response_body: Vec<u8>,
    pub created_at: SystemTime,
    pub expires_at: SystemTime,
}

/// Outcome of an atomic `reserve_or_fetch` call.
///
/// The middleware uses this state machine to decide whether to run the
/// handler, replay a cached response, or reject. Exactly one caller per
/// `(principal, key)` ever gets `Reserved` — that's the contract banking
/// flows like transfers rely on.
#[derive(Debug, Clone)]
pub enum ReservationOutcome {
    /// This caller claimed the key. It MUST run the handler and then
    /// invoke `complete` (success) or `release` (give up the
    /// reservation so a retry can re-acquire). The `token` uniquely
    /// identifies THIS reservation — `complete` and `release` only
    /// write when the row still carries the same token, so a handler
    /// that overran the TTL and had its row reclaimed by a retry
    /// can't poison the newer reservation.
    Reserved { token: uuid::Uuid },
    /// Another caller already completed an execution with the same
    /// request hash. The middleware returns the cached response.
    Replay(IdempotencyRecord),
    /// Another caller is currently executing under the same key + hash.
    /// The middleware returns `409 Conflict` with `Retry-After: 1` so
    /// the client retries shortly.
    InFlight,
    /// Same key was claimed by a different request body — the IETF
    /// draft's `idempotency_key_conflict` (422).
    Conflict,
}
