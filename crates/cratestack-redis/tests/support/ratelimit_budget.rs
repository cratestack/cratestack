//! Shared fixtures for the two cratestack#871 bucket-budget test binaries.
//!
//! `#[allow(dead_code)]` for the same reason `super::redis` carries it:
//! `mod support;` is compiled into every test binary in this crate, and
//! most of them use none of this.

#![allow(dead_code)]

use std::time::Duration;

use cratestack_core::BucketBudget;
use cratestack_redis::RedisRateLimitStore;
use uuid::Uuid;

use super::redis::TestRedis;

pub const WINDOW: Duration = Duration::from_secs(60);

/// Per-test prefix so parallel binaries (and the other Redis suites in
/// this crate) cannot trample each other, and so `SCAN <prefix>:*` counts
/// only this test's keys.
pub async fn store_or_skip(suffix: &str) -> Option<(RedisRateLimitStore, String, TestRedis)> {
    let redis = super::redis::connect_or_skip().await?;
    let prefix = format!("cratestack:test:rlb:{suffix}:{}", Uuid::new_v4().simple());
    let store = RedisRateLimitStore::from_client(redis.client.clone(), prefix.clone());
    Some((store, prefix, redis))
}

/// Every Redis key under `prefix`, cursor-walked so the assertion is on
/// the real keyspace rather than on what one `SCAN` batch happened to
/// return.
pub async fn scan_keys(redis: &TestRedis, prefix: &str) -> Vec<String> {
    let mut conn = redis
        .client
        .get_multiplexed_async_connection()
        .await
        .expect("connect");
    let mut cursor = 0u64;
    let mut found = Vec::new();
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg(format!("{prefix}:*"))
            .arg("COUNT")
            .arg(1000)
            .query_async(&mut conn)
            .await
            .expect("scan");
        found.extend(batch);
        cursor = next;
        if cursor == 0 {
            break;
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Keys in the bucket namespace only — `:rls:` scope sets excluded.
pub fn buckets(keys: &[String]) -> Vec<&String> {
    keys.iter().filter(|key| key.contains(":rl:")).collect()
}

/// Keys in the scope-set namespace only.
pub fn scopes(keys: &[String]) -> Vec<&String> {
    keys.iter().filter(|key| key.contains(":rls:")).collect()
}

pub fn budget(max_distinct: u32) -> BucketBudget {
    BucketBudget::new("peer:198.51.100.7", "ip:198.51.100.7", max_distinct, WINDOW)
}
