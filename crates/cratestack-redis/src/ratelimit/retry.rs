//! Retry-once on a connection-class failure (cratestack#846).
//!
//! `docs/design/redis-store-connection-reuse.md` (cratestack#174) made
//! both stores share one lazily-established
//! [`redis::aio::ConnectionManager`] instead of dialling per call. The
//! manager reconnects on its own, but — per its own documentation — the
//! command that *observed* the drop still fails: "that error will be
//! passed on to the user, but it will trigger a reconnection in the
//! background… all commands that are issued after the reconnect process
//! has been initiated will have to await the connection future."
//!
//! So a Redis idle-timeout or a restart costs exactly one user-visible
//! request, which is what cratestack#846 reported from production
//! (`redis rate limit: broken pipe`). Re-issuing the command once
//! absorbs it: the second attempt awaits the manager's replacement
//! connection rather than the dead one.
//!
//! ## Two deliberate limits
//!
//! **Exactly once, never a loop.** A retry budget larger than one turns a
//! Redis outage into an amplifier: every request would multiply its load
//! on an already-struggling server. One retry covers the stale-pooled-
//! connection case this exists for; anything worse is the store-error
//! policy's problem (`cratestack_axum::ratelimit::StoreErrorPolicy`).
//!
//! **Only connection-class errors.** A `NOSCRIPT`, a wrong-type error or
//! a malformed reply is deterministic — retrying it just doubles the
//! latency before the same failure. See [`is_connection_class`].
//!
//! ## Not idempotent, and that is fine here
//!
//! `consume` decrements a token. If the connection died *after* Redis
//! applied the script but before the reply reached us, the retry
//! consumes a second token. The cost is one extra token out of a bucket
//! whose whole purpose is approximate capacity protection, on a path that
//! only runs when the connection is already breaking — strictly better
//! than failing the user's request. The idempotency store gets no such
//! retry, deliberately: there, a double-apply is the exact thing it
//! exists to prevent.

use std::future::Future;

use cratestack_core::CratestackError;

use super::util::redis_error;

/// Is this the kind of failure a fresh connection can absorb?
///
/// [`redis::RedisError::is_connection_dropped`] already covers the whole
/// set that matters — `BrokenPipe`, `ConnectionReset`, `ConnectionAborted`,
/// `UnexpectedEof`, `NotConnected`, plus any error whose kind is
/// `ErrorKind::Io`. Deliberately *not* `is_timeout`: a timeout means the
/// server may still be executing the script, and re-issuing a
/// non-idempotent token consume against a server that is merely slow is
/// how a struggling Redis gets pushed over.
pub(super) fn is_connection_class(error: &redis::RedisError) -> bool {
    error.is_connection_dropped()
}

/// Runs `run` against a connection from `connect`. On a connection-class
/// failure, acquires a connection again and runs it exactly once more.
///
/// `connect` is called a second time rather than reusing the first
/// handle so that a store whose very first connection attempt failed
/// (the `OnceCell` caches no `Err`, by cratestack#174's design) gets a
/// real second chance to establish one.
pub(super) async fn invoke_with_retry<C, Connect, ConnFut, Run, RunFut, T>(
    connect: Connect,
    run: Run,
) -> Result<T, CratestackError>
where
    Connect: Fn() -> ConnFut,
    ConnFut: Future<Output = Result<C, CratestackError>>,
    Run: Fn(C) -> RunFut,
    RunFut: Future<Output = Result<T, redis::RedisError>>,
{
    let first = run(connect().await?).await;
    let error = match first {
        Ok(value) => return Ok(value),
        Err(error) if is_connection_class(&error) => error,
        Err(error) => return Err(redis_error(error)),
    };

    tracing::warn!(
        target: "cratestack",
        cratestack_operation = "rate_limit",
        error = %error,
        "redis rate limit: connection-class failure, retrying once on a fresh connection \
         (cratestack#846)",
    );
    run(connect().await?).await.map_err(redis_error)
}
