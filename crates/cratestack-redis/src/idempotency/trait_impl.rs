use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::idempotency::{IdempotencyStore, ReservationOutcome};
use cratestack_core::CoolError;
use redis::Value as RedisValue;
use uuid::Uuid;

use super::parse::{parse_reserve_outcome, value_as_string};
use super::scripts::{COMPLETE_SCRIPT, RELEASE_SCRIPT, RESERVE_SCRIPT};
use super::store::RedisIdempotencyStore;
use super::time::system_time_to_ms;
use super::util::redis_error;

#[async_trait]
impl IdempotencyStore for RedisIdempotencyStore {
    async fn reserve_or_fetch(
        &self,
        principal: &str,
        key: &str,
        request_hash: [u8; 32],
        expires_at: SystemTime,
    ) -> Result<ReservationOutcome, CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        let new_token = Uuid::new_v4();
        let expires_ms = system_time_to_ms(expires_at)?;
        let created_ms = system_time_to_ms(SystemTime::now())?;

        let value: RedisValue = RESERVE_SCRIPT
            .key(hashkey)
            .arg(request_hash.as_slice())
            .arg(new_token.as_bytes().as_slice())
            .arg(expires_ms.to_string())
            .arg(created_ms.to_string())
            .arg(principal.as_bytes())
            .arg(key.as_bytes())
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        parse_reserve_outcome(value, principal, key)
    }

    async fn complete(
        &self,
        principal: &str,
        key: &str,
        token: Uuid,
        status: u16,
        headers: &[u8],
        body: &[u8],
    ) -> Result<(), CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        // The Lua script reads `expires_at` straight off the hash so we
        // don't need a separate round-trip and the PEXPIREAT stays
        // atomic with the response write. A mismatched token short-
        // circuits before either touches Redis.
        let outcome: RedisValue = COMPLETE_SCRIPT
            .key(hashkey)
            .arg(token.as_bytes().as_slice())
            .arg(u32::from(status).to_string())
            .arg(headers)
            .arg(body)
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        match value_as_string(&outcome).as_deref() {
            // `token_mismatch` is the documented silent no-op: a stale
            // handler whose reservation got reclaimed must not surface
            // an error, otherwise the inner service sees a spurious
            // failure for a successful request.
            Some("ok") | Some("token_mismatch") => Ok(()),
            other => Err(CoolError::Internal(format!(
                "redis idempotency: unexpected complete result: {other:?}"
            ))),
        }
    }

    async fn release(&self, principal: &str, key: &str, token: Uuid) -> Result<(), CoolError> {
        let mut conn = self.connection().await?;
        let hashkey = self.hash_key(principal, key);
        let _: RedisValue = RELEASE_SCRIPT
            .key(hashkey)
            .arg(token.as_bytes().as_slice())
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;
        Ok(())
    }
}
