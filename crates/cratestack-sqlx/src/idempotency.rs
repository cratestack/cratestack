//! Postgres-backed [`IdempotencyStore`].
//!
//! Banks typically want idempotency state in the same database as the
//! mutation it guards, so a stale read or replica lag can't accidentally
//! re-execute. This implementation reads and writes via the same pool the
//! rest of the runtime uses; a deployment can swap in a Redis-backed
//! implementation by writing its own [`IdempotencyStore`].

use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::idempotency::{IDEMPOTENCY_TABLE_DDL, IdempotencyRecord, IdempotencyStore};
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

    /// Delete expired rows. Run periodically (e.g. via a scheduled task or
    /// the `cratestack idempotency gc` CLI subcommand) — Phase 1 does not
    /// auto-GC inside the request path.
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
    async fn fetch(
        &self,
        principal: &str,
        key: &str,
    ) -> Result<Option<IdempotencyRecord>, CoolError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                Vec<u8>,
                i32,
                Option<String>,
                Vec<u8>,
                chrono::DateTime<chrono::Utc>,
                chrono::DateTime<chrono::Utc>,
            ),
        >(
            "SELECT principal_fingerprint, key, request_hash, response_status,
                    response_content_type, response_body, created_at, expires_at
             FROM cratestack_idempotency
             WHERE principal_fingerprint = $1 AND key = $2 AND expires_at > NOW()",
        )
        .bind(principal)
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| CoolError::Database(error.to_string()))?;

        Ok(row.and_then(|row| {
            let request_hash: [u8; 32] = match row.2.as_slice().try_into() {
                Ok(bytes) => bytes,
                Err(_) => return None,
            };
            let response_status: u16 = u16::try_from(row.3).ok()?;
            Some(IdempotencyRecord {
                principal_fingerprint: row.0,
                key: row.1,
                request_hash,
                response_status,
                response_content_type: row.4,
                response_body: row.5,
                created_at: row.6.into(),
                expires_at: row.7.into(),
            })
        }))
    }

    async fn put(&self, record: &IdempotencyRecord) -> Result<(), CoolError> {
        let created_at: chrono::DateTime<chrono::Utc> = record.created_at.into();
        let expires_at: chrono::DateTime<chrono::Utc> = record.expires_at.into();
        sqlx::query(
            "INSERT INTO cratestack_idempotency (
                principal_fingerprint, key, request_hash, response_status,
                response_content_type, response_body, created_at, expires_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (principal_fingerprint, key) DO NOTHING",
        )
        .bind(&record.principal_fingerprint)
        .bind(&record.key)
        .bind(record.request_hash.as_slice())
        .bind(record.response_status as i32)
        .bind(record.response_content_type.as_deref())
        .bind(record.response_body.as_slice())
        .bind(created_at)
        .bind(expires_at)
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
