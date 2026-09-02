use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::{CratestackError, RateLimitConfig, RateLimitDecision, RateLimitStore};
use redis::Value as RedisValue;

use super::parse::parse_consume_outcome;
use super::retry::invoke_with_retry;
use super::scripts::CONSUME_SCRIPT;
use super::store::RedisRateLimitStore;
use super::time::system_time_to_ms;

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        let now_ms = system_time_to_ms(SystemTime::now())?;
        let bucket_key = self.bucket_key(key);
        // Owned up front so the retry closure below can be `Fn` (called
        // twice) rather than `FnOnce`.
        let now_arg = now_ms.to_string();
        let burst_arg = config.burst.to_string();
        let refill_arg = format!("{}", config.refill_per_second);

        // Lua's `tonumber` accepts standard decimal notation; we serialise
        // the float with `{:?}` so values like `0.001` round-trip through
        // Rust's `f64::to_string`-equivalent without ever taking on a
        // locale-dependent form. `tostring`/`tonumber` inside the script
        // are unaffected by Redis's locale because Lua 5.1 (which Redis
        // embeds) uses C-locale formatting.
        let value: RedisValue = invoke_with_retry(
            || self.connection(),
            |mut conn| {
                let (bucket_key, now_arg, burst_arg, refill_arg) =
                    (&bucket_key, &now_arg, &burst_arg, &refill_arg);
                async move {
                    CONSUME_SCRIPT
                        .key(bucket_key)
                        .arg(now_arg)
                        .arg(burst_arg)
                        .arg(refill_arg)
                        .invoke_async(&mut conn)
                        .await
                }
            },
        )
        .await?;

        parse_consume_outcome(value)
    }
}
