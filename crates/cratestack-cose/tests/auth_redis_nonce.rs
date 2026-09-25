//! The Redis nonce bridge against a real Redis (`auth` feature,
//! cratestack#1005 part B). Skips without one, unless
//! `CRATESTACK_REQUIRE_REDIS` is set, which makes a skip a failure (see
//! `redis_support`). Each test starts its own container; on rootless
//! Docker, use `--test-threads=1` (parallel containers race on the port
//! bind, which looks like a test failure).

mod common;
mod redis_support;

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use common::{hex, rpc_request};
use cratestack_core::{CratestackError, NonceStore};
use cratestack_cose::auth::{AUTH_NONCE_KEY_ID, AuthNonceStore};
use cratestack_cose::{CoseEnvelope, CoseMode, UNAUTHENTICATED};

/// Where auth's Redis store puts a bridged key.
fn redis_key(opener_key: &str) -> String {
    format!("cratestack:signature-nonce:{AUTH_NONCE_KEY_ID}:{opener_key}")
}

/// When `key` will expire, read from Redis: bounds `(earliest, latest)` on
/// the instant, from the clock before and after `PTTL`. `None` if the key
/// does not exist or has no expiry.
async fn expiry(
    conn: &mut redis::aio::MultiplexedConnection,
    key: &str,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let before = Utc::now();
    let pttl: i64 = redis::cmd("PTTL")
        .arg(key)
        .query_async(conn)
        .await
        .expect("PTTL");
    let after = Utc::now();
    (pttl > 0).then(|| {
        let ttl = Duration::milliseconds(pttl);
        (before + ttl, after + ttl)
    })
}

async fn wait_until_gone(conn: &mut redis::aio::MultiplexedConnection, key: &str) {
    for _ in 0..100 {
        let exists: i64 = redis::cmd("EXISTS")
            .arg(key)
            .query_async(conn)
            .await
            .expect("EXISTS");
        if exists == 0 {
            return;
        }
        tokio::time::sleep(StdDuration::from_millis(100)).await;
    }
    panic!("{key} never expired");
}

#[tokio::test]
async fn replay_is_refused_until_expiry_and_the_key_is_gone_after_it() {
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    let bridge = AuthNonceStore::redis(&redis.url).expect("url");
    let cti = uuid::Uuid::new_v4().simple().to_string();
    let key = format!("cose:0011223344556677:{cti}");
    let expires_at = Utc::now() + Duration::seconds(2);

    assert!(
        bridge
            .record_if_unseen(&key, expires_at)
            .await
            .expect("first")
    );
    assert!(
        !bridge
            .record_if_unseen(&key, expires_at)
            .await
            .expect("replay")
    );
    let distinct = format!("cose:0011223344556677:{cti}ff");
    assert!(
        bridge
            .record_if_unseen(&distinct, expires_at)
            .await
            .expect("distinct")
    );

    let mut conn = redis.connection().await;
    let (earliest, latest) = expiry(&mut conn, &redis_key(&key))
        .await
        .expect("has a TTL");
    let slack = Duration::milliseconds(5);
    assert!(
        earliest + slack >= expires_at,
        "outlives expires_at: {earliest} vs {expires_at}"
    );
    assert!(
        latest <= expires_at + Duration::milliseconds(1_250),
        "at most ~1 s past it: {latest}"
    );

    wait_until_gone(&mut conn, &redis_key(&key)).await;
    assert!(Utc::now() >= expires_at, "gone only after expires_at");
    // The store has forgotten it, as designed; from here on the opener's
    // freshness check is what refuses the message (next test).
    assert!(
        bridge
            .record_if_unseen(&key, expires_at)
            .await
            .expect("after expiry")
    );
}

fn replica(url: &str, skew_secs: u64) -> CoseEnvelope {
    CoseEnvelope::server(
        CoseMode::Sign1,
        common::signer(cratestack_cose::CoseAlg::Ed25519),
        common::resolver(),
        Arc::new(AuthNonceStore::redis(url).expect("url")),
    )
    .skew(StdDuration::from_secs(skew_secs))
    .build()
    .expect("server")
}

fn assert_coarse_401(result: Result<cratestack_cose::Opened, CratestackError>) {
    match result {
        Err(CratestackError::Unauthorized(message)) => assert_eq!(message, UNAUTHENTICATED),
        other => panic!("expected the coarse 401, got {other:?}"),
    }
}

/// Two replicas sharing one Redis, with a 2 s skew: a request opens once,
/// on either replica; a distinct `cti` opens; the `(kid, cti)` entry
/// expires at `iat + 2·skew + 1` (to within a second), and afterwards the
/// replay is still refused, by freshness.
#[tokio::test]
async fn replicas_sharing_redis_open_a_request_once() {
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    const SKEW: u64 = 2;
    let (a, b) = (replica(&redis.url, SKEW), replica(&redis.url, SKEW));
    let client = CoseEnvelope::client(
        CoseMode::Sign1,
        common::signer(cratestack_cose::CoseAlg::Ed25519),
        common::resolver(),
    )
    .build()
    .expect("client");
    let payment = common::fixture::payment_bytes();
    let sealed = client
        .seal_request(&payment, &rpc_request())
        .await
        .expect("seal");

    let opened = a
        .open_request(sealed.clone(), &rpc_request())
        .await
        .expect("first");
    assert_coarse_401(b.open_request(sealed.clone(), &rpc_request()).await);
    assert_coarse_401(a.open_request(sealed.clone(), &rpc_request()).await);
    let other = client
        .seal_request(&payment, &rpc_request())
        .await
        .expect("seal");
    b.open_request(other, &rpc_request())
        .await
        .expect("a distinct cti opens");

    let (iat, cti) = (opened.iat.expect("iat"), opened.cti.expect("cti"));
    let key = redis_key(&format!("cose:{}:{}", hex(&opened.kid), hex(&cti)));
    let mut conn = redis.connection().await;
    let (earliest, latest) = expiry(&mut conn, &key).await.expect("recorded with a TTL");
    let expires_at = DateTime::from_timestamp(i64::try_from(iat + 2 * SKEW + 1).expect("fits"), 0)
        .expect("in range");
    assert!(
        earliest + Duration::milliseconds(5) >= expires_at,
        "{earliest} vs {expires_at}"
    );
    assert!(
        latest <= expires_at + Duration::milliseconds(1_250),
        "{latest} vs {expires_at}"
    );

    wait_until_gone(&mut conn, &key).await;
    assert_coarse_401(a.open_request(sealed, &rpc_request()).await);
}
