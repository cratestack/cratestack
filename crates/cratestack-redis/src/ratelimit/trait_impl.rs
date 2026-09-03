use std::time::SystemTime;

use async_trait::async_trait;
use cratestack_core::{
    BoundedOutcome, BucketBudget, ConsumeRequest, CratestackError, RateLimitConfig,
    RateLimitDecision, RateLimitStore, bucket_ttl_secs,
};
use redis::Value as RedisValue;

use super::parse::parse_bounded_outcome;
use super::retry::invoke_with_retry;
use super::scripts::CONSUME_SCRIPT;
use super::store::RedisRateLimitStore;
use super::time::system_time_to_ms;
use super::util::scope_member;

/// `max_distinct` sentinel telling the script "no budget on this call".
/// Negative rather than, say, `0`, because 0 is a meaningful cap (admit
/// nothing, always charge the fallback) and must stay expressible.
const NO_BUDGET: i64 = -1;

#[async_trait]
impl RateLimitStore for RedisRateLimitStore {
    /// Delegates, so the two entry points cannot acquire different
    /// behaviour: there is one script and one code path, and `consume`
    /// is `consume_bounded` with no budget.
    async fn consume(
        &self,
        key: &str,
        config: RateLimitConfig,
    ) -> Result<RateLimitDecision, CratestackError> {
        self.consume_bounded(ConsumeRequest::new(key, config, None))
            .await
            .map(|outcome| outcome.decision)
    }

    async fn consume_bounded(
        &self,
        request: ConsumeRequest<'_>,
    ) -> Result<BoundedOutcome, CratestackError> {
        let now_ms = system_time_to_ms(SystemTime::now())?;
        let bucket_key = self.bucket_key(request.key);
        let keys = self.budget_keys(request.budget, now_ms, &bucket_key);
        // Owned up front so the retry closure below can be `Fn` (called
        // twice) rather than `FnOnce`.
        let args = ScriptArgs::new(request, now_ms, request.budget);

        // Lua's `tonumber` accepts standard decimal notation; we serialise
        // the float with `{:?}` so values like `0.001` round-trip through
        // Rust's `f64::to_string`-equivalent without ever taking on a
        // locale-dependent form. `tostring`/`tonumber` inside the script
        // are unaffected by Redis's locale because Lua 5.1 (which Redis
        // embeds) uses C-locale formatting.
        let value: RedisValue = invoke_with_retry(
            || self.connection(),
            |mut conn| {
                let (keys, args) = (&keys, &args);
                async move {
                    CONSUME_SCRIPT
                        .key(&keys.0)
                        .key(&keys.1)
                        .key(&keys.2)
                        .arg(&args.now_ms)
                        .arg(&args.burst)
                        .arg(&args.refill)
                        .arg(&args.ttl_sec)
                        .arg(&args.max_distinct)
                        .arg(&args.window_ms)
                        .arg(&args.member)
                        .invoke_async(&mut conn)
                        .await
                }
            },
            &self.retry_warning,
        )
        .await?;

        parse_bounded_outcome(value)
    }
}

impl RedisRateLimitStore {
    /// The script's three KEYS, in order: requested bucket, scope set,
    /// fallback bucket.
    ///
    /// With no budget the last two are copies of the first. That is not a
    /// placeholder for its own sake: passing the same key three times
    /// keeps the call single-slot under Redis Cluster for the unbudgeted
    /// path, and the script never touches KEYS[2]/KEYS[3] when
    /// `max_distinct` is negative.
    fn budget_keys(
        &self,
        budget: Option<&BucketBudget>,
        now_ms: i64,
        bucket_key: &str,
    ) -> (String, String, String) {
        let Some(budget) = budget else {
            return (
                bucket_key.to_owned(),
                bucket_key.to_owned(),
                bucket_key.to_owned(),
            );
        };
        let window_ms = window_ms(budget);
        // Fixed-window epoch, computed here rather than in Lua: the script
        // must stay deterministic for replication/AOF, and `TIME` is not
        // allowed before a write in older Redis versions.
        let epoch = now_ms.div_euclid(window_ms.max(1));
        (
            bucket_key.to_owned(),
            self.scope_key(&budget.scope_key, epoch),
            self.bucket_key(&budget.fallback_key),
        )
    }
}

/// The script's seven ARGV slots, pre-rendered as strings.
struct ScriptArgs {
    now_ms: String,
    burst: String,
    refill: String,
    ttl_sec: String,
    max_distinct: String,
    window_ms: String,
    member: String,
}

impl ScriptArgs {
    fn new(request: ConsumeRequest<'_>, now_ms: i64, budget: Option<&BucketBudget>) -> Self {
        Self {
            now_ms: now_ms.to_string(),
            burst: request.config.burst.to_string(),
            refill: format!("{}", request.config.refill_per_second),
            ttl_sec: bucket_ttl_secs(request.config).to_string(),
            max_distinct: budget
                .map_or(NO_BUDGET, |b| i64::from(b.max_distinct))
                .to_string(),
            window_ms: budget.map_or(0, window_ms).to_string(),
            member: scope_member(request.key),
        }
    }
}

/// A window of zero (or one that overflows an `i64` of milliseconds) would
/// make the epoch divisor zero and the `PEXPIRE` invalid, so it is clamped
/// to at least one millisecond — a degenerate but well-defined budget
/// rather than a script error.
fn window_ms(budget: &BucketBudget) -> i64 {
    i64::try_from(budget.window.as_millis())
        .unwrap_or(i64::MAX)
        .max(1)
}
