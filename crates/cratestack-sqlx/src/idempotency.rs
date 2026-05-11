//! Postgres-backed [`IdempotencyStore`].
//!
//! Banks need duplicate-execution protection even under concurrency, so
//! this implementation uses the atomic reservation pattern: a single
//! upsert claims the key (or surfaces the existing claim), the middleware
//! runs the handler only when it owns the reservation, then writes the
//! captured response with `complete`.
//!
//! Expired rows are unconditionally replaced on the next reservation —
//! the `ON CONFLICT DO UPDATE WHERE cratestack_idempotency.expires_at <=
//! NOW()` clause lets the new caller take over a stale row in the same
//! statement that would otherwise have hit the unique-key wall.

use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::idempotency::{
    IDEMPOTENCY_TABLE_DDL, IdempotencyRecord, IdempotencyStore, ReservationOutcome,
};
use cratestack_core::CoolError;

#[derive(Clone)]
pub struct SqlxIdempotencyStore {
    pool: sqlx::PgPool,
}

impl SqlxIdempotencyStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Ensure the table exists. Banks typically run this via their own
    /// migration tooling; we expose it here for convenience.
    pub async fn ensure_schema(&self) -> Result<(), CoolError> {
        // Multi-statement DDL (table + index) — prepared statements only
        // accept one statement at a time, so split + execute sequentially.
        for statement in IDEMPOTENCY_TABLE_DDL
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            sqlx::query(statement)
                .execute(&self.pool)
                .await
                .map_err(|error| CoolError::Database(error.to_string()))?;
        }
        Ok(())
    }

    /// Delete expired rows. Run periodically (e.g. via a scheduled task
    /// or the `cratestack idempotency gc` CLI subcommand) — the request
    /// path does not auto-GC, although `reserve_or_fetch` does take over
    /// any single expired row it tries to claim.
    pub async fn garbage_collect(&self) -> Result<u64, CoolError> {
        let result = sqlx::query("DELETE FROM cratestack_idempotency WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await
            .map_err(|error| CoolError::Database(error.to_string()))?;
        Ok(result.rows_affected())
    }
}

#[async_trait]
impl IdempotencyStore for SqlxIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError> {
        let expires_at: chrono::DateTime<chrono::Utc> = expires_at.into();
        // Generate a fresh reservation token. If our INSERT or expired-
        // row UPDATE wins, this token identifies our reservation. A
        // handler that runs past the TTL and gets reclaimed by a retry
        // will see its token replaced in-row, and any later complete/
        // release from the stale handler becomes a no-op.
        let new_token = uuid::Uuid::new_v4();
        // Single upsert that:
        //   - inserts a fresh pending row if the key is absent;
        //   - takes over the row if the existing one has expired (the
        //     `WHERE` filter on the DO UPDATE branch);
        //   - leaves the row alone otherwise.
        // The `xmax = 0` trick distinguishes a real INSERT (true) from
        // an UPDATE-on-conflict (false). PG sets xmax to the locking
        // transaction id on an UPDATE; pristine inserts read xmax = 0.
        let row: Option<(
            Vec<u8>,
            uuid::Uuid,
            Option<i32>,
            Option<String>,
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
                response_content_type = NULL,
                response_body = NULL,
                created_at = NOW(),
                expires_at = EXCLUDED.expires_at
             WHERE cratestack_idempotency.expires_at <= NOW()
             RETURNING request_hash, reservation_id, response_status, response_content_type,
                       response_body, created_at, expires_at, (xmax = 0) AS was_inserted",
        )
        .bind(principal)
        .bind(key)
        .bind(request_hash.as_slice())
        .bind(new_token)
        .bind(expires_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

        if let Some((_, token, _, _, _, _, _, _)) = row {
            // Either a fresh insert (was_inserted = true) or an expired
            // row we just reclaimed (was_inserted = false but UPDATE
            // happened). In both cases the caller owns the reservation
            // and the row carries the token we just generated.
            return Ok(ReservationOutcome::Reserved { token });
        }

        // ON CONFLICT WHERE evaluated to false (existing row is live).
        // Read it back and classify.
        let existing: Option<(
            Vec<u8>,
            Option<i32>,
            Option<String>,
            Option<Vec<u8>>,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
        )> = sqlx::query_as(
            "SELECT request_hash, response_status, response_content_type,
                    response_body, created_at, expires_at
             FROM cratestack_idempotency
             WHERE principal_fingerprint = $1 AND key = $2",
        )
        .bind(principal)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

        let Some((stored_hash, status, content_type, body, created_at, existing_expires_at)) =
            existing
        else {
            // Vanished between the upsert and the read (a concurrent GC
            // could do this in theory). Surface as InFlight so the
            // caller retries shortly rather than running the handler on
            // a state we don't fully understand.
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
                    response_content_type: content_type,
                    response_body: b,
                    created_at: created_at.into(),
                    expires_at: existing_expires_at.into(),
                }))
            }
            _ => Ok(ReservationOutcome::InFlight),
        }
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        content_type: Option<&str>,
        body: &[u8],
    ) -> Result<(), CoolError> {
        // Only completes the row we actually reserved. `reservation_id =
        // $token` is the proof; `response_body IS NULL` keeps us from
        // double-writing a finished slot. A handler that ran past its
        // TTL will find the row's `reservation_id` rotated out by the
        // retry that reclaimed it, and this UPDATE matches zero rows.
        sqlx::query(
            "UPDATE cratestack_idempotency
             SET response_status = $1,
                 response_content_type = $2,
                 response_body = $3
             WHERE principal_fingerprint = $4
               AND key = $5
               AND reservation_id = $6
               AND response_body IS NULL",
        )
        .bind(status as i32)
        .bind(content_type)
        .bind(body)
        .bind(principal)
        .bind(key)
        .bind(token)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| CoolError::Database(error.to_string()))
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
    ) -> Result<(), CoolError> {
        // Only drop our own pending row — never delete a completed one,
        // and never delete a row whose reservation has been rotated to
        // a different owner.
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
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|error| CoolError::Database(error.to_string()))
    }
}

/// Compute when a record originally captured at `created_at` will expire.
/// Pulled out for unit-test reach; the SystemTime arithmetic is otherwise
/// awkward to assert against without a clock injection point.
pub fn expiry_from(created_at: SystemTime, ttl: std::time::Duration) -> SystemTime {
    created_at + ttl
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    #[test]
    fn expiry_adds_ttl_to_creation() {
        let now = SystemTime::UNIX_EPOCH;
        let expiry = expiry_from(now, Duration::from_secs(24 * 3600));
        assert_eq!(
            expiry.duration_since(SystemTime::UNIX_EPOCH).unwrap(),
            Duration::from_secs(24 * 3600),
        );
    }
}
