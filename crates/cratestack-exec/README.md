# cratestack-exec

**L3 — Execution.** The transport-neutral middle of a CrateStack operation.

This is the `OpExecutor` [`docs/design/rpc-transport.md`][rpc] §4 has specified since
2026-05-15 and [`docs/design/layering.md`][lay] §2 named as the one layer with no members.
ADR 0015 (accepted, amended 2026-09-03) settles building it. Slice 1 — what is here today —
owns **idempotency admission** and nothing else.

You do not depend on this crate directly. It arrives transitively through whichever facade
your schema selected (`cratestack-pg`, `cratestack-api`), and the HTTP entry point stays
`cratestack_axum::idempotency::IdempotencyLayer`, which is now a thin adapter over
`OpExecutor::admit`.

## What is here

```text
use cratestack_exec::{Admission, OpAdmission, OpExecutor, OpInput};

let executor = OpExecutor::new(Some(store), Duration::from_secs(24 * 3600));

let admission = executor.admit(&OpInput {
    op: OpAdmission::from(descriptor),      // or OpAdmission::unresolved()
    principal: "sha256-of-authorization",
    idempotency_key: Some("client-supplied-key"),
    fingerprint,                            // computed by the caller — see below
    ctx: None,                              // slice 3 fills this
}).await?;
```

`Admission::Bypass` means "run the op, there is nothing to complete or release". The other
four mirror `cratestack_core::idempotency_record::ReservationOutcome` exactly.

## Two exclusions, and why the dependency list is two entries long

`layering.md` §2's L3 section forbids anything **transport-shaped** (`http::HeaderMap`,
`tower::Layer`, `axum::Response`) and anything **backend-shaped** (`sqlx::Transaction`).
Both bite here:

- `OpInput::fingerprint` is a `[u8; 32]` the *caller* computed. Method, path+query and
  content-type are transport facts. Keeping the hash at the transport is what makes
  "the wire did not change" checkable rather than assertable.
- Audit persistence cannot move here at all — it commits inside the mutation's own
  transaction (`cratestack-sqlx/src/audit.rs`), and threading `&mut Transaction` through a
  transport-neutral interface is precisely what the second exclusion forbids.

What survives both needs `cratestack-core` and `uuid`. That is the whole list.

## Not a container

`OpExecutor` holds its collaborators in named fields, supplied at construction. It resolves
nothing by type at runtime — ADR 0012, which rejects an IoC container because a type-keyed
lookup would make `examples/no-database-verification`'s `cargo tree | grep -i sqlx` proof
unstateable.

## Not in slice 1

Rate limiting (still an L4 `tower::Layer`), audit fan-out, row-level policy on
subscriptions, and `OpInput::ctx` (always `None`). `OpAdmission::rate_limited_by_default`
is carried through today so the input shape does not change when rate limiting follows.

[rpc]: https://github.com/cratestack/cratestack/blob/main/docs/design/rpc-transport.md
[lay]: https://github.com/cratestack/cratestack/blob/main/docs/design/layering.md
