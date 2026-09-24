# cratestack-exec

**L3 — Execution.** The transport-neutral middle of a CrateStack operation.

This is the `OpExecutor` [`docs/design/rpc-transport.md`][rpc] §4 has specified since
2026-05-15 and [`docs/design/layering.md`][lay] §2 named as the one layer with no members.
ADR 0015 (accepted, amended 2026-09-03) settles building it, one concern per slice. Two
are here: **idempotency admission** (slice 1) and **rate-limit admission** (slice 2,
cratestack#877).

You do not depend on this crate directly. It arrives transitively through whichever facade
your schema selected (`cratestack-pg`, `cratestack-api`), and the HTTP entry points stay
`cratestack_axum::idempotency::IdempotencyLayer` and `cratestack_axum::ratelimit::RateLimitLayer`,
which are thin adapters over `OpExecutor::admit` and `OpExecutor::admit_rate_limit`.

## What is here

```text
use cratestack_exec::{Admission, OpAdmission, OpExecutor, OpInput};

let executor = OpExecutor::new(Some(store), Duration::from_secs(24 * 3600));

let admission = executor.admit(&OpInput::new(
    OpAdmission::from(descriptor),          // or OpAdmission::unresolved()
    "sha256-of-authorization",              // principal
    Some("client-supplied-key"),
    fingerprint,                            // computed by the caller — see below
)).await?;
```

`Admission::Bypass` means "run the op, there is nothing to complete or release". The other
four mirror `cratestack_core::idempotency_record::ReservationOutcome` exactly.

```text
use cratestack_exec::{OpExecutor, OpInput, RateLimitAdmission, RateLimitBucket};

let executor = OpExecutor::new(None, Duration::ZERO).with_rate_limit(store, config);

if executor.rate_limit_applies(&op) {                       // pure; false for @no_rate_limit
    let input = OpInput::for_rate_limit(op, RateLimitBucket::new(&key, budget.as_ref()));
    match executor.admit_rate_limit(&input).await? {
        RateLimitAdmission::Consumed(outcome) => { /* outcome.decision, outcome.charged */ }
        RateLimitAdmission::Bypass => { /* not limited */ }
        _ => { /* unknown future outcome: refuse, never admit */ }
    }
}
```

Rate limiting answers with its own type, not an `Admission` variant: an *admitted* call
still carries what the response needs (`remaining`, and which bucket was charged). An op
nobody could identify (`OpAdmission::unresolved()`) is **charged**, the inverse of
idempotency, where the same doubt reserves. Both mean "when in doubt, apply the
protection".

## Two exclusions, and why the dependency list is two entries long

`layering.md` §2's L3 section forbids anything **transport-shaped** (`http::HeaderMap`,
`tower::Layer`, `axum::Response`) and anything **backend-shaped** (`sqlx::Transaction`).
Both bite here:

- `OpInput::fingerprint` is a `[u8; 32]` the *caller* computed. Method, path+query and
  content-type are transport facts. Keeping the hash at the transport is what makes
  "the wire did not change" checkable rather than assertable.
- `OpInput::rate_limit_bucket` arrives derived for the same reason: the bucket key reads
  `Authorization`, the peer address and a verified-principal extension. The lookup
  timeout, the store-error policy and the rendered `429` stay at the transport too.
- Audit persistence cannot move here at all — it commits inside the mutation's own
  transaction (`cratestack-sqlx/src/audit.rs`), and threading `&mut Transaction` through a
  transport-neutral interface is precisely what the second exclusion forbids.

What survives both needs `cratestack-core` and `uuid`. That is the whole list.

## Not a container

`OpExecutor` holds its collaborators in named fields, supplied at construction. It resolves
nothing by type at runtime — ADR 0012, which rejects an IoC container because a type-keyed
lookup would make `examples/no-database-verification`'s `cargo tree | grep -i sqlx` proof
unstateable.

## Not here yet

Audit fan-out, row-level policy on subscriptions (slice 3), and `OpInput::ctx` (always
`None`). No in-process caller (ADR 0018's `invoke_with_db` path) is wired to the executor
yet. Both admissions are reachable from HTTP only today.

[rpc]: https://github.com/cratestack/cratestack/blob/main/docs/design/rpc-transport.md
[lay]: https://github.com/cratestack/cratestack/blob/main/docs/design/layering.md
