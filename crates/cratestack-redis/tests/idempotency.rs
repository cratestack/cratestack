//! Integration tests for [`RedisIdempotencyStore`].
//!
//! Mirrors the scenarios in `crates/cratestack/tests/banking_idempotency.rs`
//! that exercise the store trait directly (no axum middleware) so we get
//! the same behavioural coverage against a live Redis. Tests are skipped
//! unless `CRATESTACK_REDIS_TEST_URL` is set — matching the sqlx test
//! crate's `CRATESTACK_TEST_DATABASE_URL` pattern rather than pulling in
//! testcontainers.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use cratestack_axum::idempotency::{IdempotencyStore, ReservationOutcome};
use cratestack_redis::RedisIdempotencyStore;
use uuid::Uuid;

fn store_or_skip(suffix: &str) -> Option<RedisIdempotencyStore> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    // Per-test prefix so parallel test binaries don't trample each other.
    // The suffix is appended below the `idem:` namespace baked into the
    // store, so two tests with different suffixes can never collide on
    // the same Redis key.
    let prefix = format!("cratestack:test:{suffix}:{}", Uuid::new_v4().simple());
    RedisIdempotencyStore::open(url, prefix).ok()
}

/// Raw `redis::Client` against the same URL — for tests that need to
/// poke Redis directly (PTTL checks, EXISTS probes, seeding).
fn raw_client_or_skip() -> Option<redis::Client> {
    let url = std::env::var("CRATESTACK_REDIS_TEST_URL").ok()?;
    redis::Client::open(url).ok()
}

#[tokio::test]
async fn reserve_then_complete_then_replay_returns_captured_response() {
    let Some(store) = store_or_skip("happy") else {
        return;
    };
    let principal = "fp-happy";
    let key = "txn-001";
    let hash = [1u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);

    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .expect("first reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected fresh reservation, got {other:?}"),
    };

    let headers = b"content-type:application/json\n".to_vec();
    let body = br#"{"transfer_id":"abc"}"#.to_vec();
    store
        .complete(principal, key, token, 201, &headers, &body)
        .await
        .expect("complete");

    let replay = store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .expect("replay reserve");
    let record = match replay {
        ReservationOutcome::Replay(record) => record,
        other => panic!("expected replay, got {other:?}"),
    };
    assert_eq!(record.response_status, 201);
    assert_eq!(record.response_headers, headers);
    assert_eq!(record.response_body, body);
    // The trait contract requires that the replayed request_hash matches
    // bit-for-bit what we reserved with — the middleware uses this to
    // detect tampering.
    assert_eq!(record.request_hash, hash);
}

#[tokio::test]
async fn second_reserve_with_same_hash_returns_in_flight() {
    let Some(store) = store_or_skip("inflight") else {
        return;
    };
    let principal = "fp-inflight";
    let key = "txn-002";
    let hash = [2u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);

    let first = store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .expect("first reserve");
    assert!(
        matches!(first, ReservationOutcome::Reserved { .. }),
        "expected Reserved, got {first:?}",
    );
    let second = store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .expect("second reserve");
    assert!(
        matches!(second, ReservationOutcome::InFlight),
        "expected InFlight, got {second:?}",
    );
}

#[tokio::test]
async fn second_reserve_with_different_hash_returns_conflict() {
    let Some(store) = store_or_skip("conflict") else {
        return;
    };
    let principal = "fp-conflict";
    let key = "txn-003";
    let hash_a = [3u8; 32];
    let hash_b = [4u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);

    store
        .reserve_or_fetch(principal, key, hash_a, expires)
        .await
        .expect("first reserve");
    let outcome = store
        .reserve_or_fetch(principal, key, hash_b, expires)
        .await
        .expect("second reserve");
    assert!(
        matches!(outcome, ReservationOutcome::Conflict),
        "expected Conflict, got {outcome:?}",
    );
}

#[tokio::test]
async fn expired_reservation_is_replaced_with_fresh_token() {
    let Some(store) = store_or_skip("expiry") else {
        return;
    };
    let principal = "fp-expiry";
    let key = "txn-004";
    let hash = [5u8; 32];

    // First reservation with a TTL already in the past. Redis evicts it
    // on the next access; the retry should observe an absent key and
    // claim a fresh reservation with a rotated token.
    let past = SystemTime::now() - Duration::from_secs(60);
    let original_token = match store
        .reserve_or_fetch(principal, key, hash, past)
        .await
        .expect("seed reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected initial reservation, got {other:?}"),
    };

    // Give Redis a beat to apply the past PEXPIREAT and evict the key.
    // PEXPIREAT with a past timestamp triggers eviction immediately, but
    // some Redis builds defer it to the next access; either way a tiny
    // sleep keeps the test deterministic across versions.
    tokio::time::sleep(Duration::from_millis(50)).await;

    let far_future = SystemTime::now() + Duration::from_secs(60);
    let retry_token = match store
        .reserve_or_fetch(principal, key, hash, far_future)
        .await
        .expect("retry reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected reclaim, got {other:?}"),
    };
    assert_ne!(
        retry_token, original_token,
        "reclaim must rotate the token — otherwise the original handler could still poison the row",
    );

    // Original handler's stale complete must be a silent no-op against
    // the newer reservation. We then verify the retry's own complete
    // sticks.
    store
        .complete(
            principal,
            key,
            original_token,
            500,
            &[],
            br#"{"stale":true}"#,
        )
        .await
        .expect("stale complete must not error");
    store
        .complete(
            principal,
            key,
            retry_token,
            201,
            &[],
            br#"{"owner":"retry"}"#,
        )
        .await
        .expect("retry complete");

    let replay = store
        .reserve_or_fetch(principal, key, hash, far_future)
        .await
        .expect("replay reserve");
    let record = match replay {
        ReservationOutcome::Replay(r) => r,
        other => panic!("expected Replay after retry complete, got {other:?}"),
    };
    assert_eq!(record.response_status, 201);
    let body_str = std::str::from_utf8(&record.response_body).expect("utf8");
    assert!(
        body_str.contains("retry") && !body_str.contains("stale"),
        "the retry's completion must win — got {body_str}",
    );
}

#[tokio::test]
async fn release_with_stale_token_does_not_delete_newer_reservation() {
    let Some(store) = store_or_skip("release") else {
        return;
    };
    let principal = "fp-release";
    let key = "txn-005";
    let hash = [6u8; 32];

    let past = SystemTime::now() - Duration::from_secs(60);
    let original_token = match store
        .reserve_or_fetch(principal, key, hash, past)
        .await
        .expect("seed reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected seed, got {other:?}"),
    };
    tokio::time::sleep(Duration::from_millis(50)).await;

    let future = SystemTime::now() + Duration::from_secs(60);
    let retry_token = match store
        .reserve_or_fetch(principal, key, hash, future)
        .await
        .expect("retry reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected reclaim, got {other:?}"),
    };
    assert_ne!(retry_token, original_token);

    // Stale release must be a no-op — the trait calls this out as the
    // banking-grade guarantee that lets the middleware safely call
    // release on every error path without worrying about overshoot.
    store
        .release(principal, key, original_token)
        .await
        .expect("stale release");

    // The retry's reservation is still active: re-reserving with the
    // same hash should report InFlight, not a fresh Reserved.
    let again = store
        .reserve_or_fetch(principal, key, hash, future)
        .await
        .expect("post-stale-release reserve");
    assert!(
        matches!(again, ReservationOutcome::InFlight),
        "stale release must not have dropped the live reservation; got {again:?}",
    );
}

#[tokio::test]
async fn release_with_matching_token_clears_reservation() {
    let Some(store) = store_or_skip("release-self") else {
        return;
    };
    let principal = "fp-release-self";
    let key = "txn-006";
    let hash = [7u8; 32];
    let future = SystemTime::now() + Duration::from_secs(60);

    let token = match store
        .reserve_or_fetch(principal, key, hash, future)
        .await
        .expect("reserve")
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected Reserved, got {other:?}"),
    };
    store.release(principal, key, token).await.expect("release");

    // After a self-release the key is gone — a re-reserve under the
    // same (principal, key, hash) must produce a fresh Reserved with a
    // brand-new token, not an InFlight or a Replay.
    let again = store
        .reserve_or_fetch(principal, key, hash, future)
        .await
        .expect("re-reserve");
    match again {
        ReservationOutcome::Reserved { token: new_token } => assert_ne!(new_token, token),
        other => panic!("expected fresh Reserved after release, got {other:?}"),
    }
}

// -----------------------------------------------------------------------------
// Isolation: different (principal, key) tuples must never collide.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn different_keys_under_same_principal_are_isolated() {
    let Some(store) = store_or_skip("iso-keys") else {
        return;
    };
    let principal = "fp-iso";
    let expires = SystemTime::now() + Duration::from_secs(60);

    let token_a = match store
        .reserve_or_fetch(principal, "txn-A", [10u8; 32], expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected Reserved for A, got {other:?}"),
    };
    let token_b = match store
        .reserve_or_fetch(principal, "txn-B", [11u8; 32], expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected Reserved for B, got {other:?}"),
    };
    assert_ne!(
        token_a, token_b,
        "distinct keys must produce distinct reservations"
    );

    // Completing A must not affect B's reservation status.
    store
        .complete(principal, "txn-A", token_a, 201, &[], b"a-body")
        .await
        .unwrap();
    let b_state = store
        .reserve_or_fetch(principal, "txn-B", [11u8; 32], expires)
        .await
        .unwrap();
    assert!(
        matches!(b_state, ReservationOutcome::InFlight),
        "B should still be in-flight after A completes; got {b_state:?}",
    );
}

#[tokio::test]
async fn same_key_under_different_principals_are_isolated() {
    let Some(store) = store_or_skip("iso-principals") else {
        return;
    };
    let key = "txn-shared";
    let expires = SystemTime::now() + Duration::from_secs(60);

    let _ = store
        .reserve_or_fetch("tenant-a", key, [20u8; 32], expires)
        .await
        .unwrap();
    let outcome = store
        .reserve_or_fetch("tenant-b", key, [20u8; 32], expires)
        .await
        .unwrap();
    // Tenant B must get its own fresh reservation even though it uses
    // the same idempotency key — multi-tenant isolation is exactly what
    // the principal fingerprint is here to provide.
    assert!(
        matches!(outcome, ReservationOutcome::Reserved { .. }),
        "tenant B should not see tenant A's reservation; got {outcome:?}",
    );
}

// -----------------------------------------------------------------------------
// TTL behaviour: PEXPIREAT actually applies, and survives across `complete`.
// -----------------------------------------------------------------------------

async fn ptll_for(client: &redis::Client, key: &str) -> i64 {
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("conn");
    redis::cmd("PTTL")
        .arg(key)
        .query_async(&mut conn)
        .await
        .expect("PTTL")
}

#[tokio::test]
async fn reservation_sets_pexpireat_in_the_future() {
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let prefix = format!("cratestack:test:pttl:{}", Uuid::new_v4().simple());
    let store = RedisIdempotencyStore::from_client(client.clone(), prefix.clone());
    let principal = "fp-pttl";
    let key = "txn-pttl";
    let hash = [30u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(120);

    let _ = store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap();

    let hashkey = store.hash_key(principal, key);
    let pttl = ptll_for(&client, &hashkey).await;
    // PTTL returns -1 if the key has no TTL and -2 if it doesn't exist.
    // We need a positive remaining-millis value within the window we
    // just set.
    assert!(
        pttl > 0 && pttl <= 120_000,
        "PTTL must be in (0, 120s]; got {pttl}",
    );
}

#[tokio::test]
async fn complete_preserves_the_reservation_pexpireat() {
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let prefix = format!("cratestack:test:pttl-complete:{}", Uuid::new_v4().simple());
    let store = RedisIdempotencyStore::from_client(client.clone(), prefix);
    let principal = "fp-pttl-c";
    let key = "txn-pttl-c";
    let hash = [31u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(120);

    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("expected Reserved, got {other:?}"),
    };
    store
        .complete(principal, key, token, 200, b"hdr", b"body")
        .await
        .unwrap();

    let hashkey = store.hash_key(principal, key);
    let pttl = ptll_for(&client, &hashkey).await;
    // After complete, the row holds the captured response and must
    // still expire — otherwise replays would survive forever and the
    // hash would grow without bound. Pre-fix versions of the script
    // would leave PTTL = -1 because HSET clears the TTL on some Redis
    // builds.
    assert!(
        pttl > 0 && pttl <= 120_000,
        "PTTL must remain in (0, 120s] after complete; got {pttl}",
    );
}

// -----------------------------------------------------------------------------
// Payload roundtripping: binary, empty, and large bodies.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn replay_returns_binary_headers_and_body_byte_for_byte() {
    let Some(store) = store_or_skip("binary") else {
        return;
    };
    let principal = "fp-bin";
    let key = "txn-bin";
    let hash = [40u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);
    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        other => panic!("got {other:?}"),
    };
    // Non-UTF-8 bytes in both headers blob and body — base64 would have
    // been needed to survive a textual encoding, but raw HSET keeps it
    // intact.
    let headers: Vec<u8> = (0u8..=255).collect();
    let body: Vec<u8> = (0u8..=255).rev().chain(0u8..=255).collect();
    store
        .complete(principal, key, token, 201, &headers, &body)
        .await
        .unwrap();

    let record = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Replay(r) => r,
        other => panic!("expected Replay, got {other:?}"),
    };
    assert_eq!(record.response_headers, headers);
    assert_eq!(record.response_body, body);
}

#[tokio::test]
async fn replay_with_empty_headers_and_body_works() {
    let Some(store) = store_or_skip("empty") else {
        return;
    };
    let principal = "fp-empty";
    let key = "txn-empty";
    let hash = [41u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);
    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        _ => panic!("Reserved expected"),
    };
    store
        .complete(principal, key, token, 204, &[], &[])
        .await
        .unwrap();
    let record = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Replay(r) => r,
        other => panic!("expected Replay, got {other:?}"),
    };
    assert_eq!(record.response_status, 204);
    assert!(record.response_headers.is_empty());
    assert!(record.response_body.is_empty());
}

#[tokio::test]
async fn replay_with_large_body_roundtrips_unchanged() {
    let Some(store) = store_or_skip("large") else {
        return;
    };
    let principal = "fp-large";
    let key = "txn-large";
    let hash = [42u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);
    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        _ => panic!("Reserved expected"),
    };
    // 256 KiB body — large enough to ensure Redis's bulk-string
    // framing actually moves real bytes, but small enough to keep the
    // test fast.
    let body: Vec<u8> = (0..(256 * 1024)).map(|i| (i % 251) as u8).collect();
    store
        .complete(
            principal,
            key,
            token,
            200,
            b"content-type:application/octet-stream",
            &body,
        )
        .await
        .unwrap();
    let record = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Replay(r) => r,
        _ => panic!("Replay expected"),
    };
    assert_eq!(record.response_body, body);
}

// -----------------------------------------------------------------------------
// Status code edge cases.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn status_code_boundaries_roundtrip() {
    let Some(store) = store_or_skip("status") else {
        return;
    };
    let expires = SystemTime::now() + Duration::from_secs(60);
    // Loop through the boundaries: 1xx (rare but legal in replay
    // contexts), 2xx, 4xx, 5xx, and the maximum HTTP-relevant code.
    for status in [100u16, 200, 422, 503, 599] {
        let principal = "fp-status";
        let key = format!("txn-status-{status}");
        let hash = [status as u8; 32];
        let token = match store
            .reserve_or_fetch(principal, &key, hash, expires)
            .await
            .unwrap()
        {
            ReservationOutcome::Reserved { token } => token,
            other => panic!("Reserved expected for {status}, got {other:?}"),
        };
        store
            .complete(principal, &key, token, status, &[], &[])
            .await
            .unwrap();
        let record = match store
            .reserve_or_fetch(principal, &key, hash, expires)
            .await
            .unwrap()
        {
            ReservationOutcome::Replay(r) => r,
            other => panic!("Replay expected for {status}, got {other:?}"),
        };
        assert_eq!(record.response_status, status);
    }
}

// -----------------------------------------------------------------------------
// Defensive store semantics: spurious or misordered calls don't corrupt.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn release_after_complete_does_not_wipe_the_cached_response() {
    // The store trait documents `release` as "give up the reservation
    // so a retry can re-acquire" — callers shouldn't invoke it after
    // `complete`, but if they do, the captured response must survive.
    // The SQL version guards this with `AND response_body IS NULL`;
    // the Redis version guards it via `status == 'in_flight'` inside
    // release.lua.
    let Some(store) = store_or_skip("release-after-complete") else {
        return;
    };
    let principal = "fp-rac";
    let key = "txn-rac";
    let hash = [50u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);
    let token = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Reserved { token } => token,
        _ => panic!("Reserved expected"),
    };
    store
        .complete(principal, key, token, 201, &[], b"survives")
        .await
        .unwrap();
    // Spurious release — must not delete the completed row.
    store.release(principal, key, token).await.unwrap();

    let record = match store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap()
    {
        ReservationOutcome::Replay(r) => r,
        other => panic!("Replay expected, got {other:?}"),
    };
    assert_eq!(record.response_body, b"survives");
}

#[tokio::test]
async fn complete_for_unreserved_key_is_silent_noop() {
    // A handler that lost its token via reclaim, or a buggy caller
    // that completes without reserving, must not surface an error —
    // the trait says token mismatches are silent.
    let Some(store) = store_or_skip("complete-no-reserve") else {
        return;
    };
    let result = store
        .complete("fp-noop", "txn-noop", Uuid::new_v4(), 200, &[], b"ignored")
        .await;
    assert!(result.is_ok(), "expected silent ok, got {result:?}");

    // And nothing was written — a subsequent reserve sees a fresh slot
    // rather than a pre-completed one.
    let expires = SystemTime::now() + Duration::from_secs(60);
    let outcome = store
        .reserve_or_fetch("fp-noop", "txn-noop", [60u8; 32], expires)
        .await
        .unwrap();
    assert!(
        matches!(outcome, ReservationOutcome::Reserved { .. }),
        "expected Reserved after no-op complete, got {outcome:?}",
    );
}

#[tokio::test]
async fn release_for_unreserved_key_is_silent_noop() {
    let Some(store) = store_or_skip("release-no-reserve") else {
        return;
    };
    let result = store.release("fp-nr", "txn-nr", Uuid::new_v4()).await;
    assert!(result.is_ok(), "expected silent ok, got {result:?}");
}

// -----------------------------------------------------------------------------
// Concurrency: exactly one Reserved across N parallel reserve_or_fetch calls.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn concurrent_reserves_elect_exactly_one_winner() {
    let Some(store) = store_or_skip("concurrent") else {
        return;
    };
    let store = Arc::new(store);
    let principal = "fp-concurrent";
    let key = "txn-concurrent";
    let hash = [70u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);

    // Fan out 16 tasks all calling reserve_or_fetch with the same key
    // and hash. The Redis Lua scripts are atomic, so exactly one must
    // observe Reserved and the rest InFlight — that's the banking-
    // grade duplicate-execution guarantee.
    let mut tasks = Vec::new();
    for _ in 0..16 {
        let store = Arc::clone(&store);
        let p = principal.to_owned();
        let k = key.to_owned();
        tasks.push(tokio::spawn(async move {
            store.reserve_or_fetch(&p, &k, hash, expires).await.unwrap()
        }));
    }
    let mut reserved = 0;
    let mut in_flight = 0;
    let mut other = 0;
    for task in tasks {
        match task.await.unwrap() {
            ReservationOutcome::Reserved { .. } => reserved += 1,
            ReservationOutcome::InFlight => in_flight += 1,
            _ => other += 1,
        }
    }
    assert_eq!(reserved, 1, "exactly one task must win — got {reserved}");
    assert_eq!(
        in_flight, 15,
        "the rest must see InFlight — got {in_flight}"
    );
    assert_eq!(other, 0, "no Conflict/Replay expected — got {other}");
}

// -----------------------------------------------------------------------------
// Configured prefix is faithfully reflected in the Redis key.
// -----------------------------------------------------------------------------

#[tokio::test]
async fn custom_prefix_is_used_for_the_redis_key() {
    let Some(client) = raw_client_or_skip() else {
        return;
    };
    let suffix = Uuid::new_v4().simple().to_string();
    let prefix = format!("custom:prefix:{suffix}");
    let store = RedisIdempotencyStore::from_client(client.clone(), prefix.clone());
    let principal = "fp-prefix";
    let key = "txn-prefix";
    let hash = [80u8; 32];
    let expires = SystemTime::now() + Duration::from_secs(60);
    let _ = store
        .reserve_or_fetch(principal, key, hash, expires)
        .await
        .unwrap();

    let expected_key = store.hash_key(principal, key);
    assert!(
        expected_key.starts_with(&format!("{prefix}:idem:")),
        "key {expected_key} should start with the configured prefix",
    );
    let mut conn = client
        .get_multiplexed_async_connection()
        .await
        .expect("conn");
    let exists: bool = redis::cmd("EXISTS")
        .arg(&expected_key)
        .query_async(&mut conn)
        .await
        .expect("EXISTS");
    assert!(exists, "Redis must hold a hash at the prefixed key");
}
