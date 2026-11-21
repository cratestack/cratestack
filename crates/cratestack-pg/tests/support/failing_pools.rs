//! Pools that fail in a known way, to prove an error site keeps what the
//! server or the driver said (cratestack#745).

use cratestack::sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use cratestack::sqlx::{ConnectOptions, PgPool};
use cratestack::{CratestackError, Value};

/// SQLSTATE the server raises for a write on a read-only connection.
pub const READ_ONLY_SQLSTATE: &str = "25006";

/// A pool whose every session is read-only: a SELECT works, any DDL or DML
/// fails with [`READ_ONLY_SQLSTATE`], and nothing about the database
/// itself has to be broken to provoke it.
pub async fn read_only_pool(url: &str) -> PgPool {
    let options: PgConnectOptions = url.parse().expect("test url parses");
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(
            options
                .options([("default_transaction_read_only", "on")])
                .log_statements(tracing::log::LevelFilter::Off),
        )
        .await
        .expect("read-only pool connects")
}

/// A pool that has been closed, so every acquire fails with `PoolClosed`,
/// the same class as a lost connection. Independent of the shared test
/// pool, which stays open.
pub async fn closed_pool(url: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(url)
        .await
        .expect("pool connects");
    pool.close().await;
    pool
}

/// A pool that serves its first `ok_acquires` acquires and then loses the
/// connection: the idle connection is discarded and the replacement cannot
/// be opened (the acquire then times out, `PoolTimedOut`), the way a dropped
/// socket looks to the caller. For a call site
/// that is not the first database step of its operation.
pub async fn pool_lost_after(url: &str, ok_acquires: usize) -> PgPool {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let acquires = Arc::new(AtomicUsize::new(0));
    let first_connect_done = Arc::new(AtomicUsize::new(0));
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(1))
        .before_acquire(move |_conn, _meta| {
            let seen = acquires.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move { Ok(seen < ok_acquires) })
        })
        .after_connect(move |_conn, _meta| {
            let earlier = first_connect_done.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if earlier == 0 {
                    Ok(())
                } else {
                    Err(cratestack::sqlx::Error::PoolClosed)
                }
            })
        })
        .connect(url)
        .await
        .expect("pool connects")
}

#[track_caller]
pub fn assert_typed(err: &CratestackError, sqlstate: &str) {
    assert!(
        matches!(err, CratestackError::DatabaseTyped(_)),
        "expected DatabaseTyped({sqlstate}), got {err:?}"
    );
    assert_eq!(err.db_sqlstate(), Some(sqlstate), "{err:?}");
}

#[track_caller]
pub fn assert_unavailable(err: &CratestackError) {
    assert!(
        matches!(err, CratestackError::Unavailable(_)),
        "expected Unavailable, got {err:?}"
    );
}

pub fn ctx() -> cratestack::CratestackContext {
    cratestack::CratestackContext::authenticated([
        ("id".to_owned(), Value::Int(7)),
        ("role".to_owned(), Value::String("admin".to_owned())),
    ])
}
