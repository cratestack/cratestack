//! Retry-once on a transport-class failure (cratestack#846).
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
//! absorbs it.
//!
//! ## What "a fresh connection" actually means here
//!
//! Calling `connection()` a second time does **not** dial a new socket:
//! the `OnceCell` is already populated, so it returns another clone of
//! the same `ConnectionManager`. The retry works for a different reason
//! than the obvious one — the manager has, on observing the first
//! failure, atomically swapped its *inner* connection future for a
//! reconnecting one, and the second attempt awaits that. The second call
//! to `connection()` is still worth making rather than reusing the first
//! handle: when the very first connection attempt failed, the cell holds
//! nothing (cratestack#174 deliberately caches no `Err`), and only a
//! second call will try to establish one at all.
//!
//! ## Three deliberate limits
//!
//! **Exactly once, never a loop.** A retry budget larger than one turns a
//! Redis outage into an amplifier: every request would multiply its load
//! on an already-struggling server.
//!
//! **Only transport-class errors** ([`super::util::is_transport_class`],
//! i.e. `RedisError::is_unrecoverable_error`). A `NOSCRIPT`, a `NOPERM`,
//! an `OOM` or a wrong-type reply is deterministic — retrying it just
//! doubles the latency before the same failure.
//!
//! **Bounded in wall-clock by the caller, not here.** Neither attempt has
//! a timeout of its own; `ConnectionManager` is configured with explicit
//! connection/response timeouts (`super::store`), and
//! `cratestack_axum::ratelimit::RateLimitLayer` additionally wraps the
//! whole `consume` — first attempt *and* retry — in one shared budget.
//! That layer-side budget is what makes "degrade to unlimited" mean
//! "degrade promptly" rather than "hang for the reconnect cycle, twice".
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
use cratestack_core::log_throttle::{LogThrottle, ThrottleDecision};

use super::util::{is_transport_class, redis_error};

/// Runs `run` against a connection from `connect`. On a transport-class
/// failure, acquires a connection again and runs it exactly once more.
///
/// `warning` is owned by the store rather than being a `static` here: a
/// Redis outage drives one retry per request at whatever rate the caller
/// chooses, so the line below is throttled — and a process-global budget
/// would make one store's outage silence another's, and make any test
/// asserting on this line order-dependent.
pub(super) async fn invoke_with_retry<C, Connect, ConnFut, Run, RunFut, T>(
    connect: Connect,
    run: Run,
    warning: &LogThrottle,
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
        Err(error) if is_transport_class(&error) => error,
        Err(error) => return Err(redis_error(error)),
    };

    if let ThrottleDecision::Emit {
        suppressed_since_last,
    } = warning.check()
    {
        tracing::warn!(
            target: "cratestack",
            cratestack_operation = "rate_limit",
            error = %error,
            suppressed_since_last,
            "redis rate limit: transport-class failure, retrying once (cratestack#846). This \
             line is throttled to one per 10s; suppressed_since_last counts the retries not \
             logged since the previous one.",
        );
    }
    run(connect().await?).await.map_err(redis_error)
}
