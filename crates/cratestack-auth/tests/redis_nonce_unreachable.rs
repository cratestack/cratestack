//! Concurrent claims against an endpoint that never answers
//! (cratestack#1070 review): each fails closed in about one connect
//! timeout, not one timeout per claim queued behind the others. No Redis
//! needed: a local listener accepts connections into its backlog and says
//! nothing, which is what a black-holed Redis looks like.

use std::time::{Duration, Instant};

use chrono::Utc;
use cratestack_auth::{AuthError, nonce_store_from_redis_url};

#[tokio::test]
async fn concurrent_claims_on_a_silent_endpoint_fail_in_about_one_connect_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let store =
        nonce_store_from_redis_url(Some(&format!("redis://127.0.0.1:{port}"))).expect("url");

    let started = Instant::now();
    let claims = (0..5).map(|i| {
        let store = store.clone();
        tokio::spawn(async move {
            let began = Instant::now();
            let result = store
                .claim(
                    "vk_1070",
                    &format!("silent-{i}"),
                    Utc::now(),
                    chrono::Duration::seconds(300),
                )
                .await;
            (result, began.elapsed())
        })
    });
    for handle in claims.collect::<Vec<_>>() {
        let (result, took) = handle.await.expect("claim task");
        assert!(
            matches!(result, Err(AuthError::NonceStoreUnavailable(_))),
            "an unreachable store fails closed, got {result:?}"
        );
        assert!(
            took < Duration::from_millis(2500),
            "one claim took {took:?}"
        );
    }
    let total = started.elapsed();
    assert!(
        total < Duration::from_secs(3),
        "5 concurrent claims took {total:?}; they must share one ~1 s connect attempt"
    );
    drop(listener);
}
