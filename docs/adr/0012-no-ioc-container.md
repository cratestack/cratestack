# ADR 0012: No IoC Container — Compile-Time Wiring Only

## Status

Accepted

## Date

2026-08-08

Context doc: [docs/design/layering.md](../design/layering.md)

## Context

`layering.md` names six strata and then documents, in §5.1, that four concerns —
policy, idempotency, rate limiting, audit — are implemented across three different
layers with no single place an operation can be seen whole. Audit persistence fires
from L2 (`cratestack-sqlx/src/audit.rs`, inside the mutation's transaction, where it
must stay); idempotency and rate limiting fire from L4 as `tower::Layer`s; row-level
`@@allow` is compiled into the SQL itself; audit fan-out (`AuditSink`) fires from
nowhere, because nothing calls it. The Controller/Service/Repository correspondence
in §6 is exact and unflattering: CrateStack has a Controller tier and a Repository
tier and no Service tier, and concerns with no legal home landed in whichever tier
their author was editing — or in none.

That is real architectural pressure, and the reflex it produces in anyone with a
Spring background is a container: register the collaborators once, resolve them by
type wherever a concern needs them, stop threading parameters. The pressure has
already produced downstream consequences — `@no_idempotency` codegen has been
blocked on `OpExecutor` across two release cycles
(`idempotency-rate-limit-declarative-surface.md` §4.2/§6, restated in `extensions.md`
§7), and `rpc-transport.md` §4 states plainly that idempotency, ratelimit and audit
"cannot remain HTTP-only `tower::Layer`s".

Three facts constrain the answer, and all three are checkable at `origin/main`
(`08fbb7e`):

1. **The dependency surface is a load-bearing guarantee, not a nicety.**
   `crates/cratestack-api/Cargo.toml` has no `cratestack-sqlx` entry at all — not
   optional, not feature-gated. `examples/no-database-verification` exists outside
   the workspace for the purpose of running `cargo tree | grep -i sqlx` and
   getting no output; the root `Cargo.toml`'s `exclude` list and the example's own
   README both explain that an in-workspace example cannot prove this, because Cargo
   unifies features across members in one session. Picking the wrong facade is a
   single `compile_error!` from `guard_server_postgres_backend`
   (`crates/cratestack-macros/src/include/datasource_guard.rs:88`).

2. **Indirection is already spent, deliberately and sparsely.** Across
   `crates/cratestack-axum/src`, excluding test files, `dyn ` appears **14 times in
   5 files**: `Arc<dyn IdempotencyStore>` (`idempotency/layer.rs:18,30`,
   `service.rs:25`) plus a `&dyn IdempotencyStore` parameter (`complete.rs:19`);
   `Arc<dyn RateLimitStore>` (`ratelimit/layer.rs:16,22,83`); two
   `Arc<dyn Fn(&Request) -> String>` key/fingerprint hooks
   (`idempotency/layer.rs:20`, `ratelimit/layer.rs:18,85`); the boxed futures
   `tower::Service` forces on an implementor (`idempotency/service.rs:41`,
   `ratelimit/layer.rs:99`); and one `Box<dyn erased_serde::Serialize>`
   (`projection.rs:57`). Every handler, route and delegate is monomorphised
   generated code.

3. **Two of the three seams have real alternatives; the third has none.**
   `SqlxIdempotencyStore` (`cratestack-sqlx/src/idempotency.rs:58`) vs
   `RedisIdempotencyStore` (`cratestack-redis/src/idempotency/trait_impl.rs:15`);
   `InMemoryRateLimitStore` (`cratestack-axum/src/ratelimit/store.rs:35`) vs
   `RedisRateLimitStore` (`cratestack-redis/src/ratelimit/trait_impl.rs:14`). No
   facade depends on `cratestack-redis` — `cratestack-pg/Cargo.toml` does not
   mention it — so the application picks the adapter and passes it in. The third
   seam, `AuditSink` (`crates/cratestack-core/src/audit.rs:81`, with `NoopAuditSink`
   at `:92` and `MulticastAuditSink` at `:113`), is **declared but unwired**: the
   only call to `record()` is `MulticastAuditSink` fanning out to its own children
   (`audit.rs:117`), `cratestack-macros` never mentions the trait, and the generated
   `CratestackBuilder` (`macros/src/include/server/runtime/postgres.rs:46-48`) has
   one field and no method to install a sink. ADR 0015 draws the consequences; here
   it matters only that the honest count of working seams is two, not three.

The question this ADR settles is therefore narrow: given that pressure, does
CrateStack buy relief with runtime, type-keyed resolution?

## Decision

**CrateStack will not adopt an IoC container, a service registry, or any
reflective, type-keyed runtime resolution of collaborators. This refusal is
permanent, not deferred.** Wiring is compile-time: a collaborator is a constructor
argument, a generic parameter, or one of a small, named, hand-audited set of
`Arc<dyn Trait>` seams — never a lookup.

Four reasons, none of which expire:

1. **It erodes monomorphisation.** CLAUDE.md's opening sentence is that a
   compile-time macro generates the typed Rust surface; fact (2) above is what that
   looks like in practice. A container resolves by type at runtime and every
   resolved edge is a virtual call the optimiser cannot see through. Fourteen `dyn`
   occurrences in five files is a budget, not a starting point.

2. **It erodes the `db = None` dependency-surface guarantee.** A registry needs its
   implementations *reachable* in order to register them. That does not make
   `examples/no-database-verification`'s `cargo tree` proof false — it makes it
   **unstateable**, which is worse, because the guarantee's whole value is that it
   is mechanically checkable by a consumer. The same argument independently kills
   any "select the database backend at runtime" design.

3. **The late binding buys nothing here.** Spring's container earns its cost when
   wiring varies without recompiling. CrateStack's schema is itself a compile-time
   input to `include_server_schema!`; there is no supported deployment in which the
   binary is fixed and the wiring varies. The rebuild was already happening.

4. **The useful 5% already exists.** "I have N implementations and choose one at
   startup" is fact (3): two working named seams, two vtables, zero reflection —
   and a third seam already shaped correctly for whenever it acquires a caller.

**This refuses containers, not dependency injection.** `Cratestack::builder(pool)`
(`macros/src/include/server/runtime/postgres.rs:51`) is constructor injection;
`.layer(Extension(...))` at the transport edge is setter injection. Both stay.

**Corollary, binding on any future L3 design.** If `OpExecutor` is built, it must be
a function over an already-chosen set of collaborators — passed in at construction,
by generic parameter or by explicit `Arc<dyn Trait>` — never a lookup against a
registry it consults. An `OpExecutor` that resolves its own collaborators is
prohibited by this ADR even though *whether to build `OpExecutor` at all* is a
separate, still-open decision (ADR 0015; `rpc-transport.md` §6.5). Likewise, how far
the Store SPI should reach is ADR 0016 and is not decided here.

This is consistent with, and is the general case of, `extensions.md` §7's already-
settled refusal of "a generic plugin/extensibility SDK for third-party extensions" —
`extension <name>` recognises a closed, framework-maintained list.

## Consequences

### Positive

- The `cargo tree` proof of absence stays stateable, and the three facades keep
  answering "what am I actually getting" statically rather than at startup.
- Wiring mistakes stay compile errors. There is no CrateStack analogue of
  `NoSuchBeanDefinitionException`, and this ADR is what keeps it that way.
- Generated code stays monomorphised, so the cost of the layer model is paid at
  `rustc` time rather than per request.
- L3's design space is constrained *before* it is designed, which is cheaper than
  constraining it afterwards.

### Negative — what this makes harder

- **Every new cross-cutting concern costs an explicit parameter.** A fourth
  operational seam means widening a public builder signature, which pre-1.0 is a
  breaking change we actually ship (cf. #453/#454, `32f89de`, 2026-08-07, which made
  `RequestAuthorizer::authorize` async and broke the public trait). The container's
  real selling point — adding a concern without touching the composition root — is
  genuinely forfeited.
- **`AuditSink` shows the cost is already being paid.** A seam that requires an
  explicit constructor parameter can sit declared-and-unwired for releases, because
  wiring it means changing a generated builder's signature. A container would have
  made it live the moment someone registered an impl. That is a real point for the
  other side and this ADR accepts it.
- **`@Transactional`-style ambient interception is permanently unavailable.** There
  is no proxy point and this ADR forbids creating one. A transaction stays a
  concrete `sqlx::Transaction` threaded through generated code.
- **Wiring stays verbose for the consumer.** Redis-backed idempotency means adding
  `cratestack-redis` yourself, constructing the store, and passing it — by design
  (no facade depends on it), but it is more typing than a starter would be.
- **Test doubles must be constructed and passed.** There is no `@MockBean`; a fake
  `AuditSink` is a struct someone writes and threads through.
- **`OpExecutor`'s signature becomes a public API surface that grows.** Refusing the
  registry does not reduce L3's cost; it arguably raises it, because the
  collaborator set has to be spelled out rather than discovered. `layering.md`
  §5.1's problem is not solved by this ADR — only bounded.

### Foreclosed

Runtime backend selection (one binary that talks to Postgres or SQLite by config);
third-party plugin ecosystems (already closed independently by `extensions.md` §7);
hot-swap or reload of framework components; any auto-configuration analogue —
Cargo features are additive only, which `cratestack-pg`'s `crypto-aws-lc-rs`
`compile_error!` documents at length in its own `Cargo.toml` (lines 121–132).

### What would make us revisit

Only one thing genuinely reopens this: a supported deployment shape in which the
wiring must vary *without a rebuild* — a distributed single binary whose backend or
concern set is chosen by configuration at start. Absent that, reason 3 holds, and
reasons 1, 2 and 4 hold regardless. Note explicitly what does **not** reopen it:
builder signatures becoming unwieldy as seams accumulate. The answer there is a
configuration struct or a typed builder, not a container.

## Alternatives considered

**A. A full IoC container — type-keyed runtime resolution.** Strongest case: it
makes L3 nearly free. New cross-cutting concerns register instead of threading, the
composition root stops growing, `AuditSink`-shaped seams do not sit unwired for
releases, and #465-class back-edges get less tempting because crates depend on the
container rather than on whichever crate happened to define a trait. For a framework
that ships thirty crates and expects to ship more bindings, that is a real
compounding saving. **Rejected** on all four reasons above; reason 2 alone is
disqualifying, because it destroys a guarantee one crate (`cratestack-api`) and two
out-of-workspace examples exist solely to provide.

**B. A narrow typemap/registry confined to L3** — `TypeId`-keyed
`Arc<dyn Any + Send + Sync>`, no proxies, no scanning, not exposed to generated
code. Strongest case: the blast radius is genuinely bounded, `http::Extensions` is
exactly this and is already in the process, and CrateStack already uses
`Extension<T>` at the transport edge, so the idiom is not foreign. **This is the
close call, and the decision is marginal at the transport edge** — `Extension<T>`
stays permitted there, because a missing extension there fails one request, and
that boundary is already dynamic. It is **rejected for the core**: inside L3 a
`TypeId` miss turns a missing collaborator from a compile error into a runtime
`None`, which is precisely the failure mode `no-database-mode.md` §7 and the
facade split exist to eliminate. The distinction this ADR draws is between the
edge (dynamic already, one request at risk) and the composition root (static, whole
process at risk).

**C. Compile-time DI by generic parameter** —
`OpExecutor<I: IdempotencyStore, R: RateLimitStore, A: AuditSink>`. **Not
rejected**; this is the permitted design and, alongside `Arc<dyn Trait>`, the shape
the corollary above contemplates. Recording its cost honestly: parameter explosion
as seams accumulate, monomorphisation bloat, and turbofish noise at every call site
that names the type. The existing seams use `Arc<dyn Trait>` rather than
generics precisely because they are cold paths where a vtable is cheaper than the
ergonomic and code-size cost — that trade-off is per-seam and stays a judgement
call, not a rule this ADR settles.

**D. Leave the question open.** Strongest case: nobody has actually proposed a
container, and refusing an unproposed thing is ceremony that ages into cargo cult.
**Rejected** because the pressure is documented and itemised rather than
hypothetical (`layering.md` §5.1), and because L3 is the next thing likely to be
designed. An unwritten refusal would have to be rediscovered during that design,
under deadline, by whoever remembers why `examples/no-database-verification` is
outside the workspace.
