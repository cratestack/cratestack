//! `nonce_store_from_redis_url`'s store against a real Redis: one managed
//! connection shared by every claim (cratestack#1070), and recovery when
//! the server drops those connections. Skips without a Redis unless
//! `CRATESTACK_REQUIRE_REDIS` is set, which makes a skip a failure (see
//! `redis_support`).
//!
//! Both tests touch server-wide state (the set of client connections, and every
//! normal client), so they take `SERIAL` and can share one
//! `CRATESTACK_REDIS_TEST_URL` under the default parallel runner. Failure
//! modes (auth change, timeouts, an unreachable server) are in
//! `redis_nonce_recovery.rs`, `redis_nonce_timeout.rs` and
//! `redis_nonce_unreachable.rs`.

mod redis_support;

use cratestack_auth::{AuthError, nonce_store_from_redis_url};

use redis_support::{SERIAL, claim, client_ids_last_running, run_tag};

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

    // Count the connections that ran the store's `SET`, by id, not the
    // server-wide `total_connections_received`: that also counts every
    // other client's connects, such as the previous test's abandoned
    // `ConnectionManager` reconnecting in the background after it killed
    // them all. Sampling after each claim also sees a connection that is
    // opened per claim and dropped straight after.
    let before = client_ids_last_running(&mut admin, "set").await;
    let mut store_connections = std::collections::BTreeSet::new();
    for i in 0..50 {
        claim(store.as_ref(), &format!("{run}-{i}"))
            .await
            .expect("fresh nonce is accepted");
        let now = client_ids_last_running(&mut admin, "set").await;
        store_connections.extend(now.difference(&before).copied());
    }
    let opened = store_connections.len();
    assert!(
        (1..=2).contains(&opened),
        "50 claims ran SET on {opened} connections ({store_connections:?}); \
         expected one shared connection, or at most a constant number"
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
