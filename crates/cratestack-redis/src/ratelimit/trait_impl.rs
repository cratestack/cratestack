use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_axum::ratelimit::{RateLimitConfig, RateLimitDecision, RateLimitStore};
use cratestack_core::CoolError;
use redis::Value as RedisValue;

use super::parse::parse_consume_outcome;
use super::scripts::CONSUME_SCRIPT;
use super::store::RedisRateLimitStore;
use super::time::system_time_to_ms;
use super::util::redis_error;

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CoolError> {
        let mut conn = self.connection().await?;
        let now_ms = system_time_to_ms(SystemTime::now())?;
        let bucket_key = self.bucket_key(key);

        // Lua's `tonumber` accepts standard decimal notation; we serialise
        // the float with `{:?}` so values like `0.001` round-trip through
        // Rust's `f64::to_string`-equivalent without ever taking on a
        // locale-dependent form. `tostring`/`tonumber` inside the script
        // are unaffected by Redis's locale because Lua 5.1 (which Redis
        // embeds) uses C-locale formatting.
        let value: RedisValue = CONSUME_SCRIPT
            .key(bucket_key)
            .arg(now_ms.to_string())
            .arg(config.burst.to_string())
            .arg(format!("{}", config.refill_per_second))
            .invoke_async(&mut conn)
            .await
            .map_err(redis_error)?;

        parse_consume_outcome(value)
    }
}
