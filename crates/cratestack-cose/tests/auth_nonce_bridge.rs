//! `AuthNonceStore`'s mapping onto `cratestack_auth::NonceStore`, without
//! Redis: the key, the expiry arithmetic, and the error mapping. The real
//! Redis run is `tests/auth_redis_nonce.rs`.

mod common;

use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, Utc};
use common::rpc_request;
use cratestack_auth::{AuthError, NonceStore as AuthStore, nonce_store_from_redis_url};
use cratestack_core::{CratestackError, NonceStore};
use cratestack_cose::CoseAlg;
use cratestack_cose::auth::{AUTH_NONCE_KEY_ID, AuthNonceStore};

type Claim = (String, String, DateTime<Utc>, Duration);

/// Records each claim, then answers with `answer`.
struct Recording {
    claims: Mutex<Vec<Claim>>,
    answer: fn() -> Result<(), AuthError>,
}

#[async_trait::async_trait]
impl AuthStore for Recording {
    async fn claim(
        &self,
        key_id: &str,
        nonce: &str,
        timestamp: DateTime<Utc>,
        replay_window: Duration,
    ) -> Result<(), AuthError> {
        self.claims.lock().expect("lock").push((
            key_id.to_owned(),
            nonce.to_owned(),
            timestamp,
            replay_window,
        ));
        (self.answer)()
    }
}

fn recording(answer: fn() -> Result<(), AuthError>) -> Arc<Recording> {
    Arc::new(Recording {
        claims: Mutex::new(Vec::new()),
        answer,
    })
}

/// The whole opener key is the auth nonce, under the fixed key id, and the
/// claimed window ends exactly one second after `expires_at` (auth's Redis
/// store rounds its TTL down, so the entry still outlives `expires_at`).
#[tokio::test]
async fn claims_the_whole_key_until_one_second_past_expires_at() {
    let auth = recording(|| Ok(()));
    let bridge = AuthNonceStore::new(auth.clone());
    let expires_at = Utc::now() + Duration::seconds(601);
    let key = "cose:0011223344556677:3c9a5e71d20b48f6a1c7e4029b6d5f83";

    let before = Utc::now();
    assert!(bridge.record_if_unseen(key, expires_at).await.expect("ok"));
    let after = Utc::now();

    let claims = auth.claims.lock().expect("lock");
    let (key_id, nonce, timestamp, window) = &claims[0];
    assert_eq!(key_id, AUTH_NONCE_KEY_ID);
    assert_eq!(nonce, key);
    assert!(
        before <= *timestamp && *timestamp <= after,
        "claimed at now"
    );
    assert_eq!(*timestamp + *window, expires_at + Duration::seconds(1));
}

#[tokio::test]
async fn nonce_reused_is_seen_and_every_other_error_is_a_500() {
    let expires_at = Utc::now() + Duration::seconds(60);
    let seen = AuthNonceStore::new(recording(|| Err(AuthError::NonceReused)));
    assert!(!seen.record_if_unseen("k", expires_at).await.expect("ok"));

    let down = AuthNonceStore::new(recording(|| {
        Err(AuthError::NonceStoreUnavailable(
            "redis at 10.9.8.7 down".to_owned(),
        ))
    }));
    let error = down
        .record_if_unseen("k", expires_at)
        .await
        .expect_err("an outage is never `new`");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    assert!(!error.public_message().contains("10.9.8.7"));
}

/// Through `cratestack_auth`'s own in-memory store: the first record is
/// new, a second is seen, a distinct key is new.
#[tokio::test]
async fn auths_in_memory_store_refuses_a_second_record() {
    let bridge = AuthNonceStore::new(nonce_store_from_redis_url(None).expect("in-memory"));
    let expires_at = Utc::now() + Duration::seconds(60);
    assert!(
        bridge
            .record_if_unseen("cose:aa:01", expires_at)
            .await
            .expect("ok")
    );
    assert!(
        !bridge
            .record_if_unseen("cose:aa:01", expires_at)
            .await
            .expect("ok")
    );
    assert!(
        bridge
            .record_if_unseen("cose:aa:02", expires_at)
            .await
            .expect("ok")
    );
}

/// An `expires_at` so far out that the bridge's deadline (`expires_at +
/// 1 s`) is not a `DateTime` is a `500`, not a panic inside auth's store
/// (whose `timestamp + replay_window` is unchecked `+`). The opener never
/// asks for this (it bounds `expires_at` by `iat + 2·skew + 1`), but the
/// bridge is a public `NonceStore`, callable directly.
#[tokio::test]
async fn an_unrepresentable_expiry_is_a_500_not_a_panic() {
    let bridge = AuthNonceStore::new(nonce_store_from_redis_url(None).expect("in-memory"));
    let error = bridge
        .record_if_unseen("cose:aa:01", DateTime::<Utc>::MAX_UTC)
        .await
        .expect_err("no deadline, no record");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    assert_eq!(error.status_code(), 500);

    // Nor does the recording store see a claim it could overflow on.
    let auth = recording(|| Ok(()));
    let error = AuthNonceStore::new(auth.clone())
        .record_if_unseen("k", DateTime::<Utc>::MAX_UTC)
        .await
        .expect_err("no deadline, no claim");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    assert!(auth.claims.lock().expect("lock").is_empty());
}

#[test]
fn an_empty_redis_url_is_refused_not_downgraded_to_in_memory() {
    for url in ["", "   "] {
        assert!(matches!(
            AuthNonceStore::redis(url),
            Err(CratestackError::Validation(_))
        ));
    }
    assert!(AuthNonceStore::redis("not a url").is_err());
}

/// A Redis that cannot be reached makes the opener answer `500`, not
/// "new" (which would switch replay protection off) and not `401`.
#[tokio::test]
async fn an_unreachable_redis_makes_opening_a_500() {
    let now = common::now();
    // Port 1 on loopback: nothing listens there, so the connection is
    // refused at once.
    let bridge = AuthNonceStore::redis("redis://127.0.0.1:1").expect("url parses");
    let server = common::server_with(CoseAlg::Ed25519, now, common::resolver(), Arc::new(bridge));
    let sealed = common::sealed_request_at(CoseAlg::Ed25519, &rpc_request(), now).await;

    let error = server
        .open_request(sealed, &rpc_request())
        .await
        .expect_err("no store, no open");
    assert!(matches!(error, CratestackError::Internal(_)), "{error:?}");
    assert_eq!(error.status_code(), 500);
}
