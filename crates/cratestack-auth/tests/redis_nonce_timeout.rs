//! A response timeout on the nonce store's connection (cratestack#1070
//! review). `CLIENT PAUSE ... ALL` holds every command for 800 ms, longer
//! than the 500 ms reply timeout and shorter than two of them.
//!
//! * A timed-out claim answers `NonceStoreUnavailable`, never
//!   `NonceReused`: a retry would queue behind the original SET on the same
//!   socket, find the key it just wrote, and turn a first use into a 401.
//! * The wedged connection is dropped: the next claim runs on a NEW server
//!   connection (a different client id), and the wedged one is gone. Judged
//!   by client id, not by `total_connections_received`, which also counts
//!   the abandoned manager's background reconnects (3 seen in CI).

mod redis_support;

use cratestack_auth::{AuthError, nonce_store_from_redis_url};
use redis_support::{SERIAL, claim, client_ids, connections_received, run_tag};

const PAUSE_MS: u64 = 800;

async fn pause(admin: &mut redis::aio::MultiplexedConnection) {
    redis::cmd("CLIENT")
        .arg("PAUSE")
        .arg(PAUSE_MS)
        .arg("ALL")
        .exec_async(admin)
        .await
        .expect("CLIENT PAUSE");
}

#[tokio::test]
async fn a_timed_out_claim_is_unavailable_never_reused() {
    let _serial = SERIAL.lock().await;
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    let mut admin = redis.connection().await;
    let store = nonce_store_from_redis_url(Some(&redis.url)).expect("url");
    let run = run_tag();
    claim(store.as_ref(), &format!("{run}-warm"))
        .await
        .expect("warm-up claim opens the shared connection");

    pause(&mut admin).await;
    let first_use = claim(store.as_ref(), &format!("{run}-slow")).await;
    assert!(
        matches!(first_use, Err(AuthError::NonceStoreUnavailable(_))),
        "a first use that times out must fail closed as unavailable, got {first_use:?}"
    );
    // Let the pause end (INFO queues behind it) before the next claim.
    connections_received(&mut admin).await;
}

#[tokio::test]
async fn a_timeout_resets_the_connection() {
    let _serial = SERIAL.lock().await;
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    let mut admin = redis.connection().await;
    let store = nonce_store_from_redis_url(Some(&redis.url)).expect("url");
    let run = run_tag();
    let idle = client_ids(&mut admin).await;
    claim(store.as_ref(), &format!("{run}-warm"))
        .await
        .expect("warm-up claim opens the shared connection");
    let warm = client_ids(&mut admin).await;
    let wedged: Vec<u64> = warm.difference(&idle).copied().collect();
    assert!(
        !wedged.is_empty(),
        "the warm-up claim must open the store's connection"
    );

    pause(&mut admin).await;
    let slow = format!("{run}-slow");
    let timed_out = claim(store.as_ref(), &slow).await;
    assert!(
        matches!(timed_out, Err(AuthError::NonceStoreUnavailable(_))),
        "the paused claim must be unavailable, got {timed_out:?}"
    );
    // The pause is over once this queued INFO answers.
    connections_received(&mut admin).await;

    claim(store.as_ref(), &format!("{run}-fresh"))
        .await
        .expect("a new nonce after the pause is accepted");
    // Without the reset the next claim reuses the wedged socket and no new
    // client id appears. With it, the store connects afresh (any extra
    // background reconnect only adds more new ids, never fewer).
    let after = client_ids(&mut admin).await;
    let fresh: Vec<u64> = after.difference(&warm).copied().collect();
    assert!(
        !fresh.is_empty(),
        "the timeout must drop the wedged connection and the next claim reconnect; \
         wedged {wedged:?}, clients now {after:?}"
    );
    assert!(
        wedged.iter().all(|id| !after.contains(id)),
        "the wedged connection {wedged:?} must be closed, clients now {after:?}"
    );

    // Replay of the timed-out nonce: the SET may have landed (the server
    // ran it after the pause: NonceReused) or not (the dropped socket
    // discarded it: Ok). Both fail closed; only an error is wrong.
    let replay = claim(store.as_ref(), &slow).await;
    assert!(
        matches!(replay, Ok(()) | Err(AuthError::NonceReused)),
        "replay after a timeout is Ok or NonceReused, got {replay:?}"
    );
}
