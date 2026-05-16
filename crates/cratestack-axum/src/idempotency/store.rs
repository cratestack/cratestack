//! `IdempotencyStore` trait + companion DDL.

use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::CoolError;

use super::record::ReservationOutcome;

/// Maximum body size the middleware will buffer when computing the hash. A
/// request beyond this returns 413 rather than risking unbounded memory.
pub(super) const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

#[async_trait]
pub trait IdempotencyStore: Send + Sync + 'static {
    /// Atomically reserve `(principal, key)` for the caller, or report
    /// the outcome of an existing reservation. Implementations MUST be
    /// concurrent-safe: two simultaneous callers seeing the same key and
    /// hash must observe exactly one `Reserved` and one `InFlight`,
    /// never two `Reserved`. The `expires_at` argument bounds the
    /// reservation's lifetime so a forgotten release doesn't pin the
    /// key forever; when a retry reclaims an expired row the store
    /// MUST rotate the reservation token so `complete`/`release` from
    /// the original handler can no longer touch the newer slot.
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError>;

    /// Persist the captured response for a previously-reserved key so
    /// subsequent attempts replay it. Banks treat the IETF idempotency
    /// contract as "freeze the outcome": if the handler returned 5xx,
    /// retries see the same 5xx unless they use a fresh key. The
    /// `token` must match the value returned by `reserve_or_fetch`
    /// when this caller claimed the key; mismatched tokens are
    /// silently no-ops so a stale handler whose reservation has been
    /// reclaimed cannot overwrite a newer execution's response.
    ///
    /// `headers` is the encoded blob from [`super::encode_headers`] —
    /// replays rebuild the response with the same `Location`, `ETag`,
    /// `Cache-Control`, `Content-Type`, etc. that the original handler
    /// set.
    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CoolError>;

    /// Release a reservation without recording a completion (e.g. the
    /// inner service panicked or the middleware itself errored before
    /// the response was ready). Subsequent attempts with the same key
    /// can re-reserve. As with `complete`, the `token` must match the
    /// active reservation.
    async fn release(&self, principal: &str, key: &str, token: uuid::Uuid)
    -> Result<(), CoolError>;
}

/// SQL DDL for the idempotency table. Banks typically run migrations through
/// their own tooling — `cratestack` currently ships migrations as raw DDL
/// since the migration engine is deferred to Phase 3.
pub const IDEMPOTENCY_TABLE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS cratestack_idempotency (
    principal_fingerprint TEXT NOT NULL,
    key TEXT NOT NULL,
    request_hash BYTEA NOT NULL,
    reservation_id UUID NOT NULL,
    response_status INT,
    response_headers BYTEA,
    response_body BYTEA,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (principal_fingerprint, key)
);

CREATE INDEX IF NOT EXISTS cratestack_idempotency_expires_idx
    ON cratestack_idempotency (expires_at);
"#;
