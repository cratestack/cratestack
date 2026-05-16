//! Postgres-backed [`IdempotencyStore`]. Banks need duplicate-execution
//! protection even under concurrency, so this uses the atomic-reservation
//! pattern: a single upsert claims the key (or surfaces the existing
//! claim), the middleware runs the handler only when it owns the
//! reservation, then writes the captured response with `complete`.
//!
//! Expired rows are replaced on the next reservation via the
//! `ON CONFLICT DO UPDATE WHERE cratestack_idempotency.expires_at <= NOW()`
//! clause, letting the new caller take over a stale row in the same
//! statement that would otherwise have hit the unique-key wall.

mod operations;

use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::idempotency::{IDEMPOTENCY_TABLE_DDL, IdempotencyStore, ReservationOutcome};
use cratestack_core::CoolError;

use crate::sqlx;

#[derive(Clone)]
pub struct SqlxIdempotencyStore {
    pool: sqlx::PgPool,
}

impl SqlxIdempotencyStore {
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self { pool }
    }

    /// Ensure the table exists. Banks typically run this via their own
    /// migration tooling; exposed here for convenience.
    pub async fn ensure_schema(&self) -> Result<(), CoolError> {
        // Multi-statement DDL (table + index) — prepared statements
        // only accept one statement at a time, so split + execute
        // sequentially.
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

    /// Delete expired rows. Run periodically — the request path does
    /// not auto-GC, although `reserve_or_fetch` does take over any
    /// single expired row it tries to claim.
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
        operations::reserve_or_fetch(&self.pool, principal, key, request_hash, expires_at).await
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CoolError> {
        operations::complete(&self.pool, principal, key, token, status, headers, body).await
    }

    async fn release(
        &self,
        principal: &str,
        key: &str,
        token: uuid::Uuid,
    ) -> Result<(), CoolError> {
        operations::release(&self.pool, principal, key, token).await
    }
}

/// Compute when a record originally captured at `created_at` will
/// expire. Pulled out for unit-test reach; the SystemTime arithmetic
/// is otherwise awkward to assert against without a clock injection
/// point.
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
