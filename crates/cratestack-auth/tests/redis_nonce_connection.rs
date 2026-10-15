//! `nonce_store_from_redis_url`'s store against a real Redis: one managed
//! connection shared by every claim (cratestack#1070), and recovery when
//! the server drops those connections. Skips without a Redis unless
//! `CRATESTACK_REQUIRE_REDIS` is set, which makes a skip a failure (see
//! `redis_support`).
//!
//! Both tests touch server-wide state (the connection counter, and every
//! normal client), so they take `SERIAL` and can share one
//! `CRATESTACK_REDIS_TEST_URL` under the default parallel runner.

mod redis_support;

use chrono::{Duration, Utc};
use cratestack_auth::{AuthError, NonceStore, nonce_store_from_redis_url};

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `total_connections_received` from `INFO stats`: every TCP connection the
/// server has accepted since it started.
async fn connections_received(conn: &mut redis::aio::MultiplexedConnection) -> u64 {
    let info: String = redis::cmd("INFO")
        .arg("stats")
        .query_async(conn)
        .await
        .expect("INFO stats");
    info.lines()
        .find_map(|line| line.strip_prefix("total_connections_received:"))
        .expect("total_connections_received in INFO stats")
        .trim()
        .parse()
        .expect("numeric connection counter")
}

async fn claim(store: &dyn NonceStore, nonce: &str) -> Result<(), AuthError> {
    store
        .claim("vk_1070", nonce, Utc::now(), Duration::seconds(300))
        .await
}

/// Distinguishes this run's nonces from an earlier run's keys still alive
/// (TTL 300 s) on a shared server.
fn run_tag() -> String {
    Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_default()
        .to_string()
}

#[tokio::test]
async fn fifty_claims_open_a_constant_number_of_connections() {
    let _serial = SERIAL.lock().await;
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    let mut admin = redis.connection().await;
    let store = nonce_store_from_redis_url(Some(&redis.url)).expect("url");
    let run = run_tag();

    let before = connections_received(&mut admin).await;
    for i in 0..50 {
        claim(store.as_ref(), &format!("{run}-{i}"))
            .await
            .expect("fresh nonce is accepted");
    }
    let opened = connections_received(&mut admin).await - before;
    assert!(
        opened <= 2,
        "50 claims opened {opened} connections; expected a constant number"
    );

    // Same store, same semantics: a replayed nonce is still refused.
    assert!(matches!(
        claim(store.as_ref(), &format!("{run}-7")).await,
        Err(AuthError::NonceReused)
    ));
}

#[tokio::test]
async fn claims_recover_after_the_server_drops_every_connection() {
    let _serial = SERIAL.lock().await;
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    let mut admin = redis.connection().await;
    let store = nonce_store_from_redis_url(Some(&redis.url)).expect("url");
    let run = run_tag();

    claim(store.as_ref(), &format!("{run}-before"))
        .await
        .expect("first claim");

    // Drop every ordinary client connection, the store's included. The
    // admin connection survives because CLIENT KILL skips the caller by
    // default (SKIPME yes).
    redis::cmd("CLIENT")
        .arg("KILL")
        .arg("TYPE")
        .arg("normal")
        .query_async::<i64>(&mut admin)
        .await
        .expect("CLIENT KILL TYPE normal");

    claim(store.as_ref(), &format!("{run}-after"))
        .await
        .expect("a fresh nonce after the drop is accepted, not an error");

    // The state lives in Redis, not in the dropped connection: a nonce
    // claimed before the drop is still refused after it.
    assert!(matches!(
        claim(store.as_ref(), &format!("{run}-before")).await,
        Err(AuthError::NonceReused)
    ));
}
