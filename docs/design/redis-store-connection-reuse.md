# Redis store connection reuse — decision

Status: **Done** (2026-07-25).
Scope: how `RedisRateLimitStore` and `RedisIdempotencyStore`
(`crates/cratestack-redis`) obtain the Redis connection they use for every
`consume`/`reserve_or_fetch`/`complete`/`release` call.
Tracking: [#174](https://github.com/cratestack/cratestack/issues/174),
[#175](https://github.com/cratestack/cratestack/pull/175).

## Decision

Both stores lazily establish a single `redis::aio::ConnectionManager` on
first use (`tokio::sync::OnceCell`) and clone it on every subsequent call,
instead of calling `redis::Client::get_multiplexed_async_connection()` fresh
on every call. A failed initial connection attempt is not cached, so the
next call retries rather than failing forever. Public constructors (`open`,
`open_with_tls`, `from_client`) stay synchronous and infallible — no API
break for existing callers.

## 1. The bug

`connection()` in both `crates/cratestack-redis/src/ratelimit/store.rs` and
`crates/cratestack-redis/src/idempotency/store.rs` called:

```rust
pub(super) async fn connection(&self) -> Result<redis::aio::MultiplexedConnection, CratestackError> {
    self.client.get_multiplexed_async_connection().await.map_err(redis_error)
}
```

— and each of the four trait methods (`consume`, `reserve_or_fetch`,
`complete`, `release`) called `self.connection()` exactly once per
invocation. `get_multiplexed_async_connection()` opens a new TCP connection
to Redis every time it's called; nothing cached or reused it. That means
every single mutating request against a `RateLimitLayer`/`IdempotencyLayer`
wrapped router opened a fresh Redis connection — in production, not just in
tests.

Found by a downstream consumer (`lightbridge-authz`) debugging intermittent
500s under `just it-tests`: a controlled repro showed direct calls to the
generated handler succeeding reliably, while the same call through the full
router with `RateLimitLayer` attached failed intermittently under
concurrent load, tracing to `CratestackError::Internal` responses from this exact
path.

## 2. Why not just cache the `MultiplexedConnection`

The obvious fix — open one `MultiplexedConnection` in `from_client()`/
`open()` and store it — has a real failure mode: `MultiplexedConnection`
does not reconnect on its own. If the TCP connection to Redis drops for any
reason (restart, network blip, idle timeout), a cached `MultiplexedConnection`
stays permanently broken for the lifetime of the store, which would be
*worse* than today's behavior (self-healing on every call, at the cost of
per-call connection overhead).

`redis::aio::ConnectionManager` (behind the `connection-manager` feature) is
the crate's own answer to exactly this tradeoff: a proxy over a multiplexed
connection that reconnects automatically in the background when a command
fails with a "connection dropped" error, and — like `MultiplexedConnection`
— is `Clone` and explicitly designed to be shared across concurrent
callers. `ConnectionManager::new(client)` is async, so it can't be called
from the stores' existing synchronous `open()`/`from_client()`; the
implementation instead wraps it in `tokio::sync::OnceCell` and defers the
actual connect to the first call to `connection()`, keeping the public
constructors unchanged (sync, infallible).

`OnceCell::get_or_try_init` does not cache an `Err` — a failed first
connection attempt leaves the cell empty, so the next call to `connection()`
tries again instead of the store failing every request forever. This is the
same self-healing property the old per-call-fresh-connection code had, just
without paying for a new TCP handshake on every request once a connection
has succeeded once.

## 3. Why this needed a custom test, not a `connected_clients` check

The first regression test written for this compared Redis's
`INFO clients: connected_clients` before/after a burst of 50 concurrent
calls. It passed against **both** the buggy and the fixed code: the old
code's per-call `MultiplexedConnection`s are opened, used once, and dropped
(closing the TCP connection) as soon as each individual `consume()`/
`reserve_or_fetch()` call finishes — which, over loopback, happens fast
enough that most of the 50 short-lived connections had already closed again
by the time the "after" snapshot was taken. Aggregate connection-count
snapshots are the wrong tool for catching per-call churn that self-closes
faster than the test can observe it.

The test that actually catches the regression tags the connection returned
by the first `connection()` call with `CLIENT SETNAME`, then asserts a
second `connection()` call round-trips through a connection with the same
name (`CLIENT GETNAME`). Confirmed to fail against the pre-fix code (the
second call landed on a fresh, unnamed connection) and pass against the
fix. This test lives inside the crate (`ratelimit/tests_store.rs`,
`idempotency/tests_store.rs`) rather than in `tests/`, because it needs
`pub(super)` access to the private `connection()` method — the public API
surface alone (`consume`/`reserve_or_fetch`) has no way to observe
connection identity.

## 4. Consequences

- Fewer TCP handshakes and less connection churn against Redis under load
  — the fix this decision exists to make.
- `cratestack-redis` gains a direct `tokio` dependency (`sync` feature, for
  `OnceCell`) alongside the existing `tokio-comp`-via-`redis` dependency,
  and the `redis` dependency gains the `connection-manager` feature.
- `RedisRateLimitStore`/`RedisIdempotencyStore` gain an `Arc<OnceCell<..>>`
  field; both remain `Clone` and their public constructors are unchanged.
- Not addressed here: `cratestack-client-store-redis` (the client-side
  `ClientStateStore` Redis implementation) is a separate concern per
  `crates/cratestack-redis/src/lib.rs`'s own module docs and was not audited
  for the same pattern as part of this change.

## Non-goals

- Connection pooling (multiple concurrent connections) — a single
  multiplexed/managed connection already supports concurrent request
  pipelining; a pool was not evaluated because the bug was "one connection
  per call," not "one connection is a throughput bottleneck."
- Making `open()`/`open_with_tls()`/`from_client()` async to connect eagerly
  — rejected because it would break every existing caller's construction
  site for no behavioral benefit over lazy-connect-on-first-use.
