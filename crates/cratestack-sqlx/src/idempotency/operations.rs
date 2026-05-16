//! Async impls for the three `IdempotencyStore` operations
//! (reserve_or_fetch / complete / release). Factored out of the entry
//! so the trait impl block stays small and the per-op SQL + comments
//! land in a focused file.

use std::time::SystemTime;

use cratestack_axum::idempotency::{IdempotencyRecord, ReservationOutcome};
use cratestack_core::CoolError;

use crate::sqlx;

pub(super) async fn reserve_or_fetch(
    pool: &sqlx::PgPool,
    principal: &str,
    key: &str,
    request_hash: [u8; 32],
    expires_at: SystemTime,
) -> Result<ReservationOutcome, CoolError> {
    let expires_at: chrono::DateTime<chrono::Utc> = expires_at.into();
    // Fresh reservation token: if our INSERT or expired-row UPDATE
    // wins, this token identifies our reservation. A handler that
    // runs past the TTL and gets reclaimed by a retry sees its token
    // replaced in-row; later complete/release from the stale handler
    // becomes a no-op.
    let new_token = uuid::Uuid::new_v4();
    // Single upsert: insert if absent, take over an expired row
    // (`WHERE` filter on DO UPDATE), leave a live row alone.
    // `xmax = 0` distinguishes a real INSERT (true) from an
    // UPDATE-on-conflict (false).
    let row: Option<(
        Vec<u8>,
        uuid::Uuid,
        Option<i32>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
        bool,
    )> = sqlx::query_as(
        "INSERT INTO cratestack_idempotency (
            principal_fingerprint, key, request_hash, reservation_id, expires_at
         ) VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (principal_fingerprint, key) DO UPDATE SET
            request_hash = EXCLUDED.request_hash,
            reservation_id = EXCLUDED.reservation_id,
            response_status = NULL,
            response_headers = NULL,
            response_body = NULL,
            created_at = NOW(),
            expires_at = EXCLUDED.expires_at
         WHERE cratestack_idempotency.expires_at <= NOW()
         RETURNING request_hash, reservation_id, response_status, response_headers,
                   response_body, created_at, expires_at, (xmax = 0) AS was_inserted",
    )
    .bind(principal)
    .bind(key)
    .bind(request_hash.as_slice())
    .bind(new_token)
    .bind(expires_at)
    .fetch_optional(pool)
    .await
    .map_err(|error| CoolError::Database(error.to_string()))?;

    if let Some((_, token, _, _, _, _, _, _)) = row {
        // Fresh insert OR expired row we just reclaimed; either way
        // we own the reservation and the row carries our token.
        return Ok(ReservationOutcome::Reserved { token });
    }

    // ON CONFLICT WHERE evaluated false (existing row is live).
    // Read it back and classify.
    let existing: Option<(
        Vec<u8>,
        Option<i32>,
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    )> = sqlx::query_as(
        "SELECT request_hash, response_status, response_headers,
                response_body, created_at, expires_at
         FROM cratestack_idempotency
         WHERE principal_fingerprint = $1 AND key = $2",
    )
    .bind(principal)
    .bind(key)
    .fetch_optional(pool)
    .await
    .map_err(|error| CoolError::Database(error.to_string()))?;

    let Some((stored_hash, status, headers, body, created_at, existing_expires_at)) = existing
    else {
        // Vanished between upsert and read (concurrent GC). Surface
        // InFlight so the caller retries rather than running on
        // state we don't understand.
        return Ok(ReservationOutcome::InFlight);
    };

    let stored: [u8; 32] = stored_hash
        .as_slice()
        .try_into()
        .map_err(|_| CoolError::Internal("corrupt idempotency hash length".to_owned()))?;
    if stored != request_hash {
        return Ok(ReservationOutcome::Conflict);
    }

    match (status, body) {
        (Some(s), Some(b)) => {
            let response_status: u16 = u16::try_from(s).unwrap_or(500);
            Ok(ReservationOutcome::Replay(IdempotencyRecord {
                principal_fingerprint: principal.to_owned(),
                key: key.to_owned(),
                request_hash: stored,
                response_status,
                response_headers: headers.unwrap_or_default(),
                response_body: b,
                created_at: created_at.into(),
                expires_at: existing_expires_at.into(),
            }))
        }
        _ => Ok(ReservationOutcome::InFlight),
    }
}

pub(super) async fn complete(
    pool: &sqlx::PgPool,
    principal: &str,
    key: &str,
    token: uuid::Uuid,
    status: u16,
    headers: &[u8],
    body: &[u8],
) -> Result<(), CoolError> {
    // Only completes the row we reserved. `reservation_id = $token`
    // is the proof; `response_body IS NULL` keeps us from
    // double-writing. A handler that ran past TTL finds its token
    // rotated out and this UPDATE matches zero rows.
    sqlx::query(
        "UPDATE cratestack_idempotency
         SET response_status = $1,
             response_headers = $2,
             response_body = $3
         WHERE principal_fingerprint = $4
           AND key = $5
           AND reservation_id = $6
           AND response_body IS NULL",
    )
    .bind(status as i32)
    .bind(headers)
    .bind(body)
    .bind(principal)
    .bind(key)
    .bind(token)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| CoolError::Database(error.to_string()))
}

pub(super) async fn release(
    pool: &sqlx::PgPool,
    principal: &str,
    key: &str,
    token: uuid::Uuid,
) -> Result<(), CoolError> {
    // Only drop our own pending row — never delete a completed one,
    // and never delete a row whose reservation has been rotated.
    sqlx::query(
        "DELETE FROM cratestack_idempotency
         WHERE principal_fingerprint = $1
           AND key = $2
           AND reservation_id = $3
           AND response_body IS NULL",
    )
    .bind(principal)
    .bind(key)
    .bind(token)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(|error| CoolError::Database(error.to_string()))
}
