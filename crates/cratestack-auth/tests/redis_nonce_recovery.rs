//! The nonce store recovers when its connection breaks for a reason the
//! driver's own reconnect does not handle: here the server's password
//! changes under it (cratestack#1070 review).
//!
//! The driver's `ConnectionManager` re-triggers a reconnect only when its
//! stored connect future failed with an I/O error. A reconnect that fails
//! with `AuthenticationFailed` stays stored, and every later claim returned
//! it until the process restarted. The store therefore drops a manager that
//! failed with anything but a server reply and builds a fresh one.
//!
//! Reconfigures the server (`CONFIG SET requirepass`), so it runs only on
//! an ephemeral container, or an external server when
//! `CRATESTACK_REDIS_TEST_ALLOW_RECONFIGURE` is set; the password is
//! restored to empty at the end, panic or not.

mod redis_support;

use cratestack_auth::{AuthError, nonce_store_from_redis_url};
use redis_support::{SERIAL, claim, run_tag};

/// Restores `requirepass ""` when dropped, whichever password is current.
struct RestorePassword(String);

impl Drop for RestorePassword {
    fn drop(&mut self) {
        for password in ["secret", "other"] {
            let url = self
                .0
                .replacen("redis://", &format!("redis://:{password}@"), 1);
            let Ok(client) = redis::Client::open(url) else {
                continue;
            };
            let Ok(mut conn) = client.get_connection() else {
                continue;
            };
            if redis::cmd("CONFIG")
                .arg("SET")
                .arg("requirepass")
                .arg("")
                .exec(&mut conn)
                .is_ok()
            {
                return;
            }
        }
    }
}

async fn set_password(admin: &mut redis::aio::MultiplexedConnection, password: &str) {
    redis::cmd("CONFIG")
        .arg("SET")
        .arg("requirepass")
        .arg(password)
        .exec_async(admin)
        .await
        .expect("CONFIG SET requirepass");
}

#[tokio::test]
async fn claims_recover_after_the_password_changes_and_is_restored() {
    let _serial = SERIAL.lock().await;
    let Some(redis) = redis_support::connect_or_skip().await else {
        eprintln!("skipping: no Redis (set CRATESTACK_REQUIRE_REDIS to make this fatal)");
        return;
    };
    if !redis.may_reconfigure() {
        eprintln!(
            "skipping: set CRATESTACK_REDIS_TEST_ALLOW_RECONFIGURE to let this test CONFIG SET requirepass"
        );
        return;
    }
    let mut admin = redis.connection().await;
    let _restore = RestorePassword(redis.url.clone());
    set_password(&mut admin, "secret").await;

    let url = redis.url.replacen("redis://", "redis://:secret@", 1);
    let store = nonce_store_from_redis_url(Some(&url)).expect("url");
    let run = run_tag();
    claim(store.as_ref(), &format!("{run}-1"))
        .await
        .expect("first claim authenticates and succeeds");

    // The password changes, then the server drops every connection: the
    // store's reconnect now fails with AuthenticationFailed.
    set_password(&mut admin, "other").await;
    redis::cmd("CLIENT")
        .arg("KILL")
        .arg("TYPE")
        .arg("normal")
        .query_async::<i64>(&mut admin)
        .await
        .expect("CLIENT KILL TYPE normal");
    let during = claim(store.as_ref(), &format!("{run}-2")).await;
    assert!(
        matches!(during, Err(AuthError::NonceStoreUnavailable(_))),
        "with the wrong password a claim fails closed, got {during:?}"
    );

    // The password is right again. A fresh store would work; so must this one.
    set_password(&mut admin, "secret").await;
    let after = claim(store.as_ref(), &format!("{run}-3")).await;
    assert!(
        after.is_ok(),
        "the store must recover without a restart, got {after:?}"
    );
    assert!(matches!(
        claim(store.as_ref(), &format!("{run}-3")).await,
        Err(AuthError::NonceReused)
    ));
}
