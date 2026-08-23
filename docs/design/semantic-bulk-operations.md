# Semantic bulk model operations — should `*_many` reach the wire?

Status: **open question, no decision made.** This document exists so the
question stops being re-asked from scratch. §9 states a recommendation;
the choice between §7's options is a maintainer decision, not one this
document makes.

## Summary

Generated model surfaces expose single-record CRUD only — `list`, `get`,
`create`, `update`, `delete` — on REST routes and on RPC's
`model.<Model>.<verb>` op ids. There is no `createMany`/`updateMany`/
`deleteMany` on either.

The interesting finding is *why*, and it is not the expected one. It is
not that bulk operations were considered and rejected as too hard. **The
hard parts are built and shipped.** `cratestack-sqlx` and
`cratestack-rusqlite` both carry a complete, item-addressed batch
primitive family — with per-item policy evaluation, per-item `@version`
preconditions, per-item audit, savepoint-scoped partial success, and a
stable response envelope. None of it is reachable from a generated route
or op id, because the commit that shipped it deferred "auto-generated
axum routes" to a follow-up that was never filed.

Two later tickets re-asked the question and answered it against a
different, less suitable pair of primitives, without rediscovering this
one.

## 1. What exists today, and what can reach it

Three distinct things get called "batch" in this codebase. Conflating
them is what caused the confusion this document records.

### 1.1 `/rpc/batch` — a transport aggregator

`POST /rpc/batch` takes N frames and returns N result frames. It is
**purely transport-level**: requests go into an envelope, the envelope
always succeeds, and the response envelope reports each request's real
status. Internally each frame is handled as a separate individual
request.

Confirmed in `crates/cratestack-macros/src/include/server/rpc_module/batch.rs`:

- The dispatch loop calls `rpc_dispatch_inner(frame_state, frame_headers,
  &frame.op, ...)` per frame — the *same* dispatcher a unary call uses,
  with a full independent `authenticate()` + policy + dispatch each time.
- Each frame receives `headers.clone()` — a byte-identical copy of the
  outer HTTP request's headers.
- The response is built with `StatusCode::OK` hardcoded, wrapped in
  `Ok(responses)`. The envelope succeeds regardless of frame outcomes.

So it buys **one round trip** and **per-frame error isolation**, and
nothing else. It is not atomic, does not do one policy pass, does not
produce set-based SQL, and — because every frame sees the same cloned
`HeaderMap` — **cannot express N different `If-Match` values**. Batching
versioned updates through it would not be unsupported; it would be
silently wrong, since all N records would be checked against whichever
single precondition the caller happened to set.

This is by design, and already written down: `docs/design/rpc-transport.md:437`
lists as an explicit non-goal — *"In-batch transactional mode. Each batch
frame is its own tx."*

### 1.2 Predicate-based bulk mutate — `update_many` / `delete_many`

`crates/cratestack-sqlx/src/query/write/update_many.rs` and siblings.
One filter, one shared patch, applied to however many rows match.

Policy compiles **once** into the SQL `WHERE` clause. Rows the policy
does not admit simply are not matched, which means implicit partial
success with **no signal distinguishing "denied by policy" from "did not
match the filter"** — both are absent from the result.

It refuses optimistic locking outright, and says so
(`update_many.rs:6`): *"No `if_match` slot — bulk updates aren't an
optimistic-locking idiom."* Every matched row is unconditionally
version-bumped.

**Not referenced anywhere in `cratestack-macros`.** Reachable only from
hand-written Rust, e.g. inside a `procedure` body.

### 1.3 Item-addressed batch — `batch_create` / `batch_update` / `batch_delete` / `batch_get` / `batch_upsert`

`crates/cratestack-sqlx/src/query/batch/`, mirrored in
`crates/cratestack-rusqlite/src/batch/`. N distinct payloads, per-item
results.

This is the family that answers the hard questions, and it already
exists:

- **Per-item expected version.** `batch/update.rs:17` —
  `pub type BatchUpdateItem<PK, I> = (PK, I, Option<i64>);`. The third
  element is the expected version, enforced per item inside its own
  SAVEPOINT. *This is precisely what `/rpc/batch` structurally cannot
  express.*
- **Per-item policy.** `batch/create_item.rs` evaluates `@@allow`/`@@deny`
  once per item, inside a per-item closure. Explicit partial success:
  each item carries its own `Ok`/`Forbidden`.
- **Per-item audit and events**, dispatched only after the outer
  transaction commits, so items that roll back leave no audit row and no
  outbox entry.
- **A designed response envelope.** `crates/cratestack-core/src/batch.rs`
  defines `BatchItemStatus<T>` (49), `BatchItemError` (60),
  `BatchSummary` (80), `BatchResponse<T>` (90) — outer `Result` for
  whole-batch failures such as the size cap, per-item `Ok`/`Error`
  inside, `index`-keyed so callers can pair results to inputs.

**Also not referenced anywhere in `cratestack-macros`.** Verified:
`grep -rl "batch_create\|batch_update\|batch_delete\|batch_upsert"
crates/cratestack-macros/src` returns zero files, as does the same grep
for `update_many`/`delete_many`.

## 2. Why nobody knew this was here

`cratestack-core/src/batch.rs`'s own doc comment anticipates the binding
that never came, naming `POST /<model>/batch-*` request bodies as its
intended use.

The commit that shipped these primitives (#18, v0.3.2) stated the
deferral plainly: *"Auto-generated axum routes are deferred to a
follow-up… apps can hand-roll a thin handler against the ORM today."*
**No follow-up issue was ever filed.** A tracker search for the primitive
names returns only an unrelated SQLSTATE bug.

The question then resurfaced twice, and both times was evaluated against
the wrong pair of primitives:

- **#569** (react-admin study, closed/superseded): *"this needs a
  decision: N calls, the batch endpoint, or exposing the bulk ops."*
- **#571** (`@cratestack/refine`, shipped v0.7.16): same framing, with an
  acceptance criterion requiring bulk ops *"implemented or explicitly
  declined with a reason."*

Both weighed `update_many`/`delete_many` (§1.2) and `/rpc/batch` (§1.1).
Neither mentions §1.3. The decision that shipped — N sequential unary
round trips, recorded honestly in `packages/cratestack-refine/src/index.ts`
and `rpc-provider.ts` — was reasoned, documented, and made on incomplete
information.

`docs/design/rpc-batching-coalescing-model.md` is not about this: it
specifies *client-side transport coalescing* for `@cratestack/link-batch`
and explicitly scopes out server-side batch changes.

## 3. What a semantic `*_many` would buy over `/rpc/batch`

Stated precisely, because "batch already covers it" is the tempting wrong
answer:

| | `/rpc/batch` | semantic `*_many` via §1.3 |
|---|---|---|
| One HTTP round trip | yes | yes |
| Per-item error isolation | yes | yes |
| One transaction | **no** — frame per tx | yes, with per-item SAVEPOINT rollback |
| One policy evaluation pass | **no** — N full dispatches | per-item, but no repeated auth/dispatch overhead |
| Per-record `If-Match` | **structurally impossible** | yes (`Option<i64>` per item) |
| Per-request overhead (auth, connection, framing) | paid N times internally | paid once |

Note what the right column does *not* claim: `batch_create`/`batch_update`
still issue N statements under SAVEPOINTs, not a single multi-row
`INSERT`. The win is transactional and overhead-related, not
set-based-SQL.

## 4. Policy: two shipped answers, deliberately different

The question "does a bulk op evaluate policy N times or once, and what
happens when it admits 8 of 10" already has two answers in tree, and they
are different on purpose:

- §1.2 (predicate-based): **once**, compiled into the `WHERE`. Implicit
  partial success, no per-row explanation.
- §1.3 (item-addressed): **N times**, one per item. Explicit partial
  success with per-item status.

Any binding must choose which it exposes, or expose both. They are not
interchangeable: the first cannot report *why* a row was skipped, and the
second cannot express "update everything matching this filter."

## 5. Audit and `@@emit`

Per record, both ways, confirmed in `update_many_exec.rs` and
`batch/create_item.rs`. Nothing in the codebase contemplates a single
aggregate audit row or event for a bulk operation. A semantic `*_many`
would emit N audit rows and N events.

If an aggregate audit record is ever wanted, that is a separate design
question and should not be smuggled in with a wire binding.

## 6. Idempotency and rate limiting

`/rpc/batch` **rejects** a shared `Idempotency-Key` header outright
(`batch.rs:33-39`): *"Idempotency-Key header is not supported on
/rpc/batch; use the per-frame `idem` field instead."*

That is useful precedent: a bulk surface needs **per-item** idempotency,
not a request-level header. Any binding designed here should follow it.

Rate limiting is dispatch-side, so `/rpc/batch` charges N units for N
frames. A genuine bulk op has no natural "N dispatches" to charge
against and would need an explicit policy — most plausibly N units
decided up front from the item count, bounded by `BATCH_MAX_ITEMS`.

## 7. Transport options

### 7.1 RPC — cheap

`model.<Model>.createMany` fits the existing dotted op-id scheme, and the
dispatch machinery already exists to extend. The cost is entirely in the
semantics above, not in transport plumbing.

### 7.2 REST — no precedent to reuse

There is no bulk verb in the current design and no convention to follow.
`POST /<model>/bulk`, `POST /<model>/batch-create`, or a `PATCH` on the
collection are all defensible. `cratestack-core/src/batch.rs`'s doc
comment already suggests `POST /<model>/batch-*`, which at least has the
virtue of being written down first.

Whoever takes this decides the verb from scratch. That is the single
largest open design question here.

### 7.3 gRPC

Out of scope — protobuf/gRPC support was removed in 0.8.5.

## 8. The client-side payoff is smaller than it looks

Checked against `@refinedev/core`'s own types
(`contexts/data/types.d.ts:252`):

```ts
export interface UpdateManyResponse<TData = BaseRecord> {
    data: TData[];
}
```

**No per-item error slot.** So even if §1.3 were exposed as routes,
`@cratestack/refine` would still have to collapse `BatchResponse<T>`'s
per-item envelope into either "throw if any item failed" or "silently
drop failures" — discarding exactly the partial-success information the
server already computes.

And the shapes do not line up. refine's `UpdateManyParams` (`:302`) is
`{ resource, ids, variables }` — **one shared patch across N ids**. That
maps naturally onto §1.2's predicate-based `update_many` (`id IN (...)`),
which is the primitive that refuses per-record `@version`. It does *not*
map onto §1.3's N-distinct-patches-with-N-distinct-versions shape.

So exposing the good primitive would not automatically improve refine.
**This closes an ORM-ergonomics gap more than a refine-UX gap**, and any
issue framing it as "make refine's bulk ops fast" would be misleading.

## 9. Recommendation

Not a decision — the choice below is the maintainer's.

This is a **half-finished feature with a lost thread**, not an unexplored
one. The expensive parts are done and tested. What remains is a wire
binding and the REST verb question.

I would suggest, in order of preference:

1. **Bind §1.3 to RPC op ids only** (`model.<M>.createMany` etc.), leaving
   REST alone for now. It is the cheap half, it has no verb question, and
   it makes the already-built per-item `@version` support reachable —
   which is the one capability nothing else in the system provides.
2. **Then decide REST separately**, with the verb question given the
   attention it deserves rather than settled as a footnote to the RPC
   work.
3. **Do not frame either as a refine improvement.** §8 is the honest
   framing: the server capability is real, the client contract cannot
   fully consume it.

**Declining is also defensible** — the primitives have served fine as
ORM-level APIs for many releases, and hand-rolling a procedure over them
is a supported path. But if that is the call, it belongs recorded here,
next to `rpc-transport.md`, rather than left as an unfiled deferral a
second time. That is precisely how this was lost once already.

## 10. Files worth reading before acting

- `crates/cratestack-core/src/batch.rs` — the pre-built envelope types
- `crates/cratestack-sqlx/src/query/batch/*` and
  `crates/cratestack-rusqlite/src/batch/*` — both backends' implementations
- `crates/cratestack-sqlx/src/query/write/update_many.rs` — the
  predicate-based alternative and its documented `@version` refusal
- `crates/cratestack-macros/src/include/server/rpc_module/batch.rs` —
  `/rpc/batch`'s transport-only dispatch loop
- `packages/cratestack-refine/src/{index.ts,rpc-provider.ts,rpc-many.ts}` —
  the shipped client-side decision and its stated reasoning
- Issues #18 (primitives shipped, deferral stated), #569 and #571 (the
  decision points), #488 (open epic on internal/non-HTTP data-layer
  ergonomics — adjacent context)
