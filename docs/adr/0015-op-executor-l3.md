# ADR 0015: Whether to Build the L3 OpExecutor Now

## Status

**Accepted (amended)** — 2026-09-03. Accepted at alternative **(a) build L3 now**,
not at the Decision as proposed. The original Decision text is retained below as
the historical record; where the two conflict, this amendment governs.

## Date

2026-08-08 (proposed); **2026-09-03 (accepted, amended — maintainer-delegated,
[cratestack#875 comment][decision])**

Context doc: [docs/design/layering.md](../design/layering.md)

## Context

`rpc-transport.md` §4 has specified an `OpExecutor` since 2026-05-15: idempotency,
rate limiting and audit "cannot remain HTTP-only `tower::Layer`s. They move into a
small `OpExecutor` service in `cratestack-core` (or a new crate) that takes
`(op_id, idem_key, request_bytes, principal)` and runs the op." §6.5 gates building
it on "a concrete bidirectional/multiplexing case" for the WebSocket binding.
`git grep -l OpExecutor origin/main` returns four paths — `CHANGELOG.md`,
`docs/design/{rpc-transport,extensions,idempotency-rate-limit-declarative-surface}.md`
— and **zero** lines under `crates/`. In [layering.md](../design/layering.md)'s
vocabulary this is L3, and it is empty.

The gate was written as "wait for WebSocket". WebSocket has not appeared, but the
cost of the empty layer is now itemisable without it. Verified against `origin/main`
(`08fbb7e`):

1. **Two participation booleans already ship on `OpDescriptor`, and nothing reads
   them.** `idempotent_by_default` and `rate_limited_by_default`
   (`crates/cratestack-core/src/transport.rs:47,57`) are emitted by
   `crates/cratestack-macros/src/transport/op_descriptors.rs:178-179`. Every
   non-test reader is the emitter itself; the only consumers are assertions in
   `crates/cratestack-pg/tests/include_schema.rs:2447` and
   `crates/cratestack-pg/tests/rate_limit_extension.rs:46`. `RateLimitLayer`
   (`crates/cratestack-axum/src/ratelimit/layer.rs:15-19`) is built from
   `(store, config, key_fn: Arc<dyn Fn(&Request) -> String>)` — it is handed an
   `http::Request`, never an `OpDescriptor`, so it cannot honour
   `rate_limited_by_default` even in principle. **`@no_rate_limit` therefore parses,
   validates, gates on `extension rate_limit { }`, carries an integration test, and
   does nothing at runtime.** The field's own doc comment admits it
   (`transport.rs:54-56`): "changes nothing about whether `RateLimitLayer` is
   actually wired up at runtime."
   **Superseded 2026-09-03 — see Amendment below:** #474 shipped the runtime
   readers (`cratestack-axum/src/ratelimit/{rest,rpc}_ops_filter.rs`), installed via
   `RateLimitLayer::with_should_rate_limit_fn`. `@no_rate_limit` takes effect
   wherever that layer is installed.
2. **`@no_idempotency` has been blocked on `OpExecutor` since before it was
   written down.** Parsed
   (`crates/cratestack-parser/src/tests_procedures.rs:157`), documented at
   `crates/cratestack-axum/src/idempotency/mod.rs:38-40` as Phase 1 being opt-in with
   "a follow-up will wire it into macro-generated routers by default, gated by a
   `@no_idempotency` opt-out attribute already recognised by the parser", and
   consumed by nothing in `cratestack-macros` (grep: zero hits).
   `idempotency-rate-limit-declarative-surface.md` §6 defers its ticket explicitly
   "gated on `OpExecutor`" and adds "this should not be opened until `OpExecutor` has
   a concrete plan".
3. **Row-level `@@allow` is compiled into SQL** by
   `crates/cratestack-sqlx/src/query/support/policy.rs` and so is not replayed
   against streamed `ModelEvent<T>` items (`rpc-transport.md` §3.4a, restated as a
   scope limit in §6.5). A subscriber authenticates but gets no per-row filtering.
4. **Cross-cutting protections attach to router *instances*, and — at the time this
   ADR was written — there were two.** `transport grpc` built a second `axum::Router`
   via `crates/cratestack-macros/src/include/server/grpc/service.rs:187`;
   `trusted-proxy-client-ip.md` decision 6 recorded that a consumer who layered
   protection onto `router()`'s output left gRPC as exposed as before. Commit
   `08fbb7e` (#416/#459) was the concrete instance: a spoofable-header fix applied
   inside `IdempotencyLayer` and `RateLimitLayer`, i.e. once per Layer, and
   therefore only on the routers the consumer remembered to layer. **Superseded
   2026-08-18:** `transport grpc` and its second router were removed entirely (ADR
   0017), so this specific instance of the risk no longer exists — there is exactly
   one router again. The general risk this fact illustrated (a second router
   instance being missed by a `.layer(...)` fix) is still real in principle for any
   future binding that builds its own router rather than reusing `router()`; this
   ADR does not know of a live instance of it today.

Two corrections this ADR's argument depends on, both verified, and both now folded
back into [layering.md](../design/layering.md) so the two documents agree:

- **`AuditSink` is a declared seam with no consumer.**
  `git grep AuditSink origin/main -- crates/ examples/` finds the trait
  (`crates/cratestack-core/src/audit.rs:81`), its two in-tree impls, one README line
  and one doc cross-reference. The only call to `record()` is `MulticastAuditSink`
  fanning out to its own children (`audit.rs:117`). `cratestack-macros` contains
  **zero** mentions of it, and `CratestackBuilder`
  (`crates/cratestack-macros/src/include/server/runtime/postgres.rs:46-48`) has
  exactly one field, `SqlxRuntime`, and no method that accepts a sink.
  **Superseded 2026-09-03 — see Amendment below:** #473 added
  `SqlxRuntime::with_audit_sink` / `CratestackBuilder::with_audit_sink` and the
  post-commit dispatch at `cratestack-sqlx/src/audit/sink.rs:82`.
- **"Audit fires from L2" is not a misplacement.**
  `crates/cratestack-sqlx/src/audit.rs:1-5` states the guarantee — "Audit rows
  write inside the mutation's transaction — you can never see a committed row whose
  audit entry didn't also commit" — and `enqueue_audit_event` is called with
  `&mut *tx` from every writer under `query/write/*` and `query/batch/*`. Audit
  *persistence* is at L2 because it must be. Only fan-out could move, and fan-out has
  no caller.

**This ADR also amends two shipped documents**, per this repo's convention that a
reframing is recorded rather than left to go stale (`extensions.md` §9):

- `extensions.md` §5 says the `rate_limit` Cargo feature "gates the dispatch-layer
  codegen that reads `rate_limited_by_default`". There is no such reader, and fact (1)
  is why. That sentence should be corrected to describe what shipped: the feature
  unlocks the attribute and threads the flag onto the descriptor, and nothing consumes
  it yet.
  **Superseded 2026-09-03 — see Amendment below:** the instruction above is itself
  now wrong in its second half. There is still no *codegen* reader, but there are
  runtime readers (#474), so "nothing consumes it yet" must not be written into
  `extensions.md`. The correction actually applied by the accepting PR says the
  feature unlocks the **attribute** — the field is emitted unconditionally — and
  names the ops filters as the consumers.
- `rpc-transport.md` §4's and
  `idempotency-rate-limit-declarative-surface.md` §4.2's statements of the
  `OpExecutor` gate should point here for the restated form below.

## Decision

### Amendment 2026-09-03 — the gate fired, and three facts went stale

**Deferred trigger 1 fired.** The restated gate — "L3 gets built when a dispatch
path must make an admission decision from an input that is not an `http::Request`"
— is met, and not by WebSocket. ADR 0018 (Accepted 2026-09-02, v0.11.0) makes the
in-process invocation path (`invoke_with_db`, `db.transaction(...)`,
`db.queries().<name>().run(...)`) a first-class, compatibility-committed dispatch
path whose input is a typed `Args` struct and a `CratestackContext`, never an
`http::Request`. It has real consumers: `examples/db-transaction-verification`
(#513) and `examples/declarative-query-verification` (#867), both re-run by CI's
`facade-disjointness` job, plus the #486 estate. **N = 2 input shapes exist today**,
which is exactly the condition alternative (a) was rejected for lacking.

**Three facts in *Context* are stale as of `8ee25402` and are corrected here rather
than edited in place:**

1. **Fact 1 is obsolete.** `rate_limited_by_default` *is* read at runtime, by
   `cratestack-axum/src/ratelimit/{rest,rpc}_ops_filter.rs` (#474, closed), wired
   through `RateLimitLayer::with_should_rate_limit_fn` and driven by
   `cratestack-pg/tests/rate_limit_runtime.rs`. `@no_rate_limit` is no longer inert,
   and the corresponding *Negative* consequence and the `extensions.md` §5
   correction it demanded are both void. This does not weaken the case for L3 — it
   strengthens it: #474 shipped the substance of alternative (c) for one of the two
   flags (a resolver closure over the static table rather than the matched
   descriptor), proving the descriptor-lookup mechanism works, and leaving
   idempotency as the only concern still unable to see the op it is about to run.
2. **Correction 1 is obsolete.** `AuditSink` has a consumer:
   `SqlxRuntime::with_audit_sink` / `CratestackBuilder::with_audit_sink` (#473),
   dispatched post-commit from `cratestack-sqlx/src/audit/sink.rs:82` and asserted
   by `cratestack-pg/tests/banking_audit.rs`. Alternative (b)'s rejection ("fan-out
   has no call site") no longer holds on those grounds; it is still not part of this
   acceptance, because fan-out already fires from the correct place.
3. **The `mcp` question is answered: no.** `mcp { }` parses
   (`cratestack-parser/src/parse/mod.rs:179`) as an inert config block.
   `cratestack-macros` and `cratestack-axum` contain zero references to it. It
   dispatches nothing and is not the trigger.

**Constraints carried forward unchanged from the original Decision.** L3 is a
function over an already-chosen set of collaborators, never a registry (ADR 0012).
Audit *persistence* stays at L2 inside the mutation's transaction. Nothing threads
`&mut Transaction` through L3. `examples/no-database-verification`'s proof of
absence stays stateable, and `db = None` keeps working — the L3 crate's only
workspace dependency is `cratestack-core`.

**Sequencing.** L3 lands in slices, one concern at a time, each with byte-identical
wire behaviour as its acceptance bar. Slice 1 is idempotency (it is the only
concern still blocked) and is tracked by **#876**. Slice 2 is rate limiting (#877),
and must wait for #871, which is rewriting `ratelimit/` now. Slice 3 is row-level
policy replay for subscriptions (fact 3). The WS binding remains the eventual
fourth caller, no longer the trigger. The epic is **#875**.

### Original Decision (2026-08-08, retained as historical record)

**CrateStack will not build the L3 `OpExecutor` in this cycle.** The
`rpc-transport.md` §6.5 gate holds, but it is **restated in layer terms rather than
transport terms**: L3 gets built when a dispatch path must make an admission
decision (policy, idempotency, rate limit) from an input that is **not an
`http::Request`**. WebSocket is one such path; it is no longer the only qualifying
one, and it is no longer required.

The corollary matters as much as the rule, and this ADR must be honest about what
happened to its own supporting example. **Original text (as of this ADR's proposal,
2026-08-08):** "The corollary matters as much as the rule: **gRPC does not qualify.**
`grpc::into_router()` returns an `axum::Router` whose handlers call the same seven
generated dispatch functions (`trusted-proxy-client-ip.md` decision 6, confirmed
against `include/server/grpc/service.rs`). Two router instances is a wiring problem,
not a layering one, and building L3 to solve it would be solving the wrong problem."

**Note added 2026-08-18, prior to this ADR's acceptance or rejection:** `transport
grpc` was removed in 0.8.5 (ADR 0017), and with it the only surface in this codebase
that ever exercised the corollary above — no other binding, past or currently
planned, builds a second router instance whose handlers reuse the same generated
dispatch functions. A substitute example was sought and not found: REST and RPC
both dispatch through the *same* `router()` call already, so neither one is a
plausible "looks like it might need L3 but doesn't" case the way gRPC's separate
`into_router()` was — there is no live ambiguity for the corollary to resolve today.
Rather than invent a hypothetical binding to stand in for gRPC, this section is left
as a historical record of the argument's original shape. The rule itself — L3 gets
built when a dispatch path's input is not an `http::Request` — does not depend on
this corollary and is unaffected; only the illustration that a superficially
plausible counterexample turns out not to qualify has lost its example.

When L3 is built it will be **a function over an already-chosen set of
collaborators, never a registry** — per ADR 0012, a type-keyed runtime lookup would
make `examples/no-database-verification`'s proof of absence unstateable.

This ADR decides **only** whether to build L3 now. Making
`idempotent_by_default`/`rate_limited_by_default` enforceable is a different
decision — see *Alternatives considered* (c) — and needs its own ticket.

**What the maintainer must decide, and what would settle it.** Three questions,
each with a cheap answer:

- *Is `@no_rate_limit` doing nothing acceptable for another cycle?* Settled by one
  decision: either accept it and say so in `extensions.md` §5 (which currently implies
  otherwise), or treat it as a bug and take (c).
- *Does the restated gate actually change anything?* Settled by naming one
  candidate non-HTTP dispatch path with a real consumer. The `mcp` block is the
  only plausible one in-tree today; if it is going to dispatch ops, the gate is
  already met and this ADR should be reopened before it is merged.
- *Is the eventual OpExecutor getting more expensive by waiting?* Settled by
  measuring: `860c08b` (gRPC) touched 121 files, +11,221/−58; `c0a76d1` (SSE #390)
  touched 34 files across seven crates. (`860c08b`'s binding, `transport grpc`, was
  itself removed by ADR 0017 in 0.8.5 — the commit stats are unaffected, cited here
  only as a historical upper bound on how expensive adding a new transport binding
  can get, not as evidence gRPC still exists.) If a fourth binding is planned inside
  two cycles, the delay argument inverts and L3 should be built first.

## Consequences

### Positive

- No abstraction designed against one imagined caller. Both current dispatch
  paths — REST and RPC-over-HTTP — take an `http::Request`. (A third,
  gRPC-over-axum, existed when this ADR was proposed and was removed in 0.8.5 —
  ADR 0017; it took an `http::Request` too, so its removal does not change the
  count of *distinct* input shapes, only the count of bindings sharing one.) An
  executor factored today would be validated against exactly one input shape while
  claiming to be neutral across N, which is the failure mode the §6.5 gate exists
  to prevent.
- Audit keeps its transactional guarantee. Any L3 that owned audit *persistence*
  would have to either thread `&mut Transaction` through a transport-neutral
  interface — which [layering.md](../design/layering.md)'s L3 exclusion forbids — or
  write the audit row outside the mutation's transaction, silently downgrading
  `crates/cratestack-sqlx/src/audit.rs`'s stated invariant.
- The `db = None` dependency-surface proof stays stateable
  (`no-database-mode.md` §7).

### Negative

- **A shipped `.cstack` attribute stays inert.** `@no_rate_limit` will continue to
  parse, validate, and have no effect. That is worse than an unimplemented feature:
  it is a schema declaration a user can reasonably read as an enforcement
  guarantee. This ADR chooses to carry that for at least one more cycle, and
  `extensions.md` §5 must say so out loud rather than implying a reader exists.
- `@no_idempotency` codegen stays blocked, and its follow-up ticket
  (`idempotency-rate-limit-declarative-surface.md` §6) stays unopenable by its own
  terms.
- Row-level `@@allow` stays unenforced on subscriptions, and the gap widens with
  every binding added.
- **The eventual migration gets more expensive, monotonically.** Every new
  cross-cutting fix lands against the `tower::Layer` shape (`08fbb7e` is this
  month's) and will have to be re-landed against L3 later. Nothing here forecloses
  L3 structurally, but it does raise its price.
- The restated gate is *weaker* than "wait for WebSocket" and could be read as
  license to build L3 opportunistically. It is not: "not an `http::Request`" is a
  factual test about a real caller, not a design preference.

### Deferred

Revisit immediately on any of:

1. A dispatch path whose input is not an `http::Request` acquires a real consumer —
   the WS frame loop (`rpc-transport.md` §3.4), an in-process queue consumer, or
   `mcp` tool dispatch.
2. A second security fix has to be applied per-Layer-per-router, i.e. a repeat of
   `08fbb7e`'s shape. One occurrence is a wiring problem; two is a layering one.
3. A user-visible bug is filed that `@no_rate_limit` has no effect — at which point
   this stops being a deferred cost and becomes a correctness defect.

### Amended 2026-09-03

The *Positive* bullet "no abstraction designed against one imagined caller" is
retained but re-scoped: it is now satisfied rather than protective. L3 is designed
against two shipped input shapes with CI-enforced consumers, and the first slice's
regression oracle is the existing HTTP test suite, unedited.

The *Negative* bullets survive as an accepted cost of the slicing, not of deferral:
the `@no_rate_limit` bullet is void (#474), the `@no_idempotency` bullet is closed
by slice 1, the row-level `@@allow` bullet stays open until slice 3, and "the
eventual migration gets more expensive, monotonically" is now a live constraint on
slice ordering — #871 lands before slice 2 so its rewrite of
`ratelimit/key_fn.rs`/`store.rs` is not re-landed against L3.

## Alternatives considered

**(a) Build the full `OpExecutor` now.** Strongest case: the cost is already
itemisable without WebSocket — four consequences above, two of them user-visible
schema attributes — and a transport-neutral middle is the *only* place row-level
policy could ever be replayed for subscriptions, because L2 (where the policy SQL
lives) is not on the SSE path at all. This is the closest alternative and the
decision against it is **marginal**. It is rejected on one ground only: neutrality
across N transports cannot be designed from one transport's input shape, and today
N = 1. The itemised costs argue for *un-gating sooner*, which is what the restated
gate does; they do not establish that the design inputs exist.

**(b) Build L3 for audit and idempotency only, leaving rate limiting at L4.**
Strongest case: those two are the concerns furthest from their nominal home, and a
two-concern executor is a fraction of the full design. Rejected on the evidence in
*Context*: audit persistence must stay inside the mutation transaction, and audit
fan-out (`AuditSink`) has no call site anywhere in the workspace — so "move audit to
L3" is either a correctness regression or the relocation of dead code. That leaves
idempotency alone, and one concern is not a layer; it is a refactor of one
`tower::Layer`.

**(c) Make the two Layers descriptor-aware, without building L3.** Give
`IdempotencyLayer`/`RateLimitLayer` access to the matched `&'static OpDescriptor`
and have them honour `idempotent_by_default`/`rate_limited_by_default`. Strongest
case: it un-blocks both attributes and `@no_idempotency` codegen, closes the
inert-attribute defect, and costs a fraction of L3 — and it does not compete with
L3, since a future executor would read the same two booleans. This is genuinely
attractive and is **recommended as the follow-up ticket**, but it is a different
decision from the one this ADR is scoped to, and it should not be smuggled in as an
implication of "don't build L3". It needs its own ADR or dev ticket, including the
non-trivial question of how a `tower::Layer` positioned *above* routing learns which
op it is about to dispatch.

**(d) Delete `idempotent_by_default` and `rate_limited_by_default`.** Strongest
case: a flag nothing reads is a worse artefact than no flag, and deleting them
would make the missing layer honest rather than half-declared. Rejected: they are
the correct target shape — `rpc-transport.md` §2.2 reserved `idempotent_by_default`
before any of this existed — and removing them is churn that would have to be
undone by (a) or (c).

[epic]: https://github.com/cratestack/cratestack/issues/875
[decision]: https://github.com/cratestack/cratestack/issues/875#issuecomment-5524011041
