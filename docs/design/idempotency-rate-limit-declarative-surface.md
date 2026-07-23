# Idempotency & rate limiting on the declarative surface — decision

Status: **partially superseded** (2026-07-23) — the rate-limiting finding
below (§4.1) has been reframed by [extensions.md](extensions.md), which
proposes a `rate_limit` **extension** that declares capability-participation
only (never tunable numbers). The idempotency finding (§4.2 onward) is
unaffected and remains the current decision.
Scope: whether `@@idempotent`/`@@rate_limit(...)`-style `.cstack` attributes should
join `@@audit`/`@@soft_delete`/`@@paged`, or whether `IdempotencyLayer`/`RateLimitLayer`
stay imperative Rust middleware.
Tracking: [#139](https://github.com/cratestack/cratestack/issues/139).

## Decision

| Concern | Decision | Why (one line) |
|---|---|---|
| Rate limiting | **Keep imperative — permanently.** No `@@rate_limit(...)`. | Numeric limits are environment-tuned operational policy, not model shape, and `.cstack` values compile into `pub const`s — retuning under incident load would mean a recompile + redeploy. |
| Idempotency | **Not now — deferred, not rejected.** A boolean opt-out flag (mirroring the already-parsed `@no_idempotency`) is plausible, but sequencing it before the RPC `OpExecutor` consolidation (already scoped in `docs/design/rpc-transport.md` §4) risks building syntax against a runtime path that's about to be restructured. | See §4.2 and §5. |

Neither concern gets new `.cstack` syntax in this change. This document is the
"documented decision" the issue's acceptance criteria ask for; §6 proposes the
properly-scoped follow-up if/when idempotency declarativeness is picked back up.

## 1. The question

`@@audit`, `@@soft_delete`, and `@@paged` are declarative `.cstack` model
attributes. `IdempotencyLayer` and `RateLimitLayer` are imperative Rust
`tower::Layer`s wired up by hand in the consuming application. A backend
integrator flagged the inconsistency as real but explicitly speculative:
should idempotency/rate-limiting move onto the same declarative surface?

## 2. What's already declarative, and why it works there

All three existing model attributes are validated in
`crates/cratestack-parser/src/validate/model_attributes.rs` and are
**argument-free booleans** — `@@paged`, `@@audit`, `@@soft_delete` must appear
bare; any `(...)` argument is a parse error. (Contrast `@@retain(days: N)` in
the same file, which does take a parameter — the grammar supports config, these
three just don't use it.) Each compiles down to a flag or column name baked
into the model's static `ModelDescriptor` (`crates/cratestack-sql/src/descriptor/mod.rs`):
`audit_enabled: bool`, `soft_delete_column: Option<&'static str>`. Generated
runtime code in `cratestack-sqlx` branches on those fields — e.g.
`push_scoped_conditions` (`query/support/conditions.rs`) injects a
`deleted_at IS NULL` predicate into every read when `soft_delete_column` is
`Some`, and every write path checks `descriptor.audit_enabled` to decide
whether to open a transaction and enqueue an audit row.

The reason this works declaratively: **the fact being declared is a permanent
property of the model's data shape**, true in every environment, that other
generated code needs to know about at compile time to get the SQL right (an
extra WHERE clause, an extra column, a different response envelope for
`@@paged`). Nobody needs "soft delete, but only in staging" or "audit, but
retune the redaction list per deploy without recompiling." Where actual
per-environment tuning exists — which `AuditSink` to use, where redacted
events go — that stays in Rust (`NoopAuditSink`/`MulticastAuditSink`,
constructed at app startup), not in the schema. The schema only declares the
*shape*; wiring stays imperative even for the concerns that are otherwise
declarative. That split is the load-bearing precedent for this decision.

## 3. What's already imperative, and why it looks that way today

`IdempotencyLayer::new(store, ttl)` and `RateLimitLayer::new(store, config)`
(`crates/cratestack-axum/src/{idempotency,ratelimit}/layer.rs`) take:

- a `store: Arc<dyn IdempotencyStore>` / `Arc<dyn RateLimitStore>` — backed by
  three different implementations across three crates (in-memory in
  `cratestack-axum`, Postgres in `cratestack-sqlx`, Redis in
  `cratestack-redis`), selected by which crate the app links against and how
  it's constructed at startup;
- a `Duration` TTL / `RateLimitConfig { burst: u32, refill_per_second: f64 }`;
- an overridable key/fingerprint closure (`Arc<dyn Fn(&Request) -> String>`)
  for tenant- or principal-scoped bucketing.

None of that is a fact about a model's rows. It's operational policy that
varies by deployment tier, load profile, and incident response — the same
axis that keeps `AuditSink` selection imperative even though `@@audit` itself
is declarative.

## 4. The tradeoff, concern by concern

### 4.1 Rate limiting — keep imperative, permanently

Two independent reasons, either one sufficient on its own:

1. **No data-shape analog.** `@@audit`/`@@soft_delete`/`@@paged` each answer a
   yes/no question about how a model's rows behave that's true everywhere the
   schema is deployed. "This model is rate-limited to N req/s" isn't a fact
   about the model — it's a fact about *this deployment's* traffic budget.
   There's no boolean equivalent to fall back on the way idempotency has one
   (see §4.2); the config *is* the whole feature.
2. **`.cstack` compiles to Rust consts.** Every existing declarative attribute
   ends up as a `pub const` token stream generated by
   `cratestack-macros::model::descriptor`. A `@@rate_limit(burst: 100,
   refill: 10.0)` attribute would compile the same way — meaning dropping a
   limit from 100 to 10 during an active incident would require editing the
   schema, recompiling, and redeploying the service, instead of today's
   env-var-driven `RateLimitConfig` read once at process startup. That's a
   strictly worse operational story for a control that exists specifically
   for incident response.

This is a permanent "no," not a "not yet" — nothing about a future
`OpExecutor` consolidation (§5) changes either reason above.

### 4.2 Idempotency — a plausible flag, but the timing is wrong

Idempotency is different from rate limiting in one respect: whether a given
mutation is *safe to declare idempotent* genuinely is closer to a fact about
the operation's semantics than a tunable — much like `@@audit`/`@@soft_delete`
are facts about a model. The codebase already has a placeholder for exactly
this: `@no_idempotency` is recognized by `cratestack-parser`
(`crates/cratestack-parser/src/tests_procedures.rs`) as a procedure attribute,
documented in `cratestack-axum/src/idempotency/mod.rs` as inert plumbing for a
"Phase 2" default-on wiring — but nothing in `cratestack-macros` consumes it
yet. The TTL/store/key-fn config would **not** move into the schema even in
this design — only the boolean "does this procedure participate in
idempotency" would, exactly mirroring how `@@audit` declares participation
while `AuditSink` selection stays in Rust.

The reason to defer rather than build this now: `docs/design/rpc-transport.md`
§4 has already identified that idempotency (along with rate-limiting and
audit) "cannot remain HTTP-only `tower::Layer`s" once the WebSocket binding
ships, and specifies moving them into a shared `OpExecutor` service in
`cratestack-core` that both the HTTP and WS dispatchers call into. That
consolidation is unbuilt (§6.5 of that doc: WS/subscriptions are pending, no
driving use case yet). The RPC vocabulary already reserves a field for this —
`OpDescriptor.idempotent_by_default: bool` (§2.2) — which is precisely the
shape a declarative `@no_idempotency` flag would need to feed. Building the
schema attribute and its codegen wiring *before* `OpExecutor` exists means
targeting the current HTTP-Layer-only runtime and likely redoing the wiring
once the executor lands. Sequencing the other way — executor first, flag
second — means the flag has exactly one runtime path to wire into instead of
two.

## 5. What the flag would look like, when it's time

Not building this now, but recording the shape so the follow-up ticket
doesn't start from scratch:

- **Where:** procedure-level attribute (idempotency is a per-call-semantics
  question, not a per-model one) — `mutation procedure createPayment(...)
  @no_idempotency` for opt-out, no opt-in attribute needed if mutations
  default to idempotent (matching `OpDescriptor.idempotent_by_default`).
- **Validation:** same bare/argument-free pattern as `@@audit`/`@@soft_delete`
  in `validate_model_attributes` (or its procedure-level analog) — reject any
  `(...)` argument, exactly like today's `@@audit` validation rejects
  `@@audit(...)`.
- **Codegen:** the boolean lands on the procedure's descriptor next to
  `OpDescriptor.idempotent_by_default`, read by whichever dispatcher (REST
  today, REST+WS after the executor consolidation) decides whether to run the
  op through the idempotency path at all.
- **Explicitly not in scope for the flag:** TTL, store selection, key/
  fingerprint function — those stay Rust-level, constructed at startup,
  exactly as `AuditSink` is today.

## 6. Proposed follow-up (not scoped for implementation here)

If/when picked back up, the properly scoped ticket is:

> **Title:** Add `@no_idempotency` codegen wiring, gated on `OpExecutor`
> **Depends on:** the `OpExecutor` consolidation from
> `docs/design/rpc-transport.md` §4 (itself gated on a concrete WS/subscription
> driving case per that doc's §6.5).
> **Scope:** consume the already-parsed `@no_idempotency` attribute in
> `cratestack-macros`, thread `idempotent_by_default` onto generated
> `OpDescriptor`s, and have the (post-consolidation) executor skip the
> idempotency path when the flag is set. No rate-limiting work — §4.1 closes
> that permanently. No TTL/store/key-fn attributes — those stay imperative
> per §5.
> **Out of scope:** anything not gated on `OpExecutor` existing.

This should not be opened until `OpExecutor` has a concrete plan, per §4.2.

## 7. Non-goals

- `@@rate_limit(...)` carrying tunable numbers (burst, refill, window) —
  still closed permanently, see §4.1. Superseded only on the narrower
  question of a participation-only `rate_limit` extension with no
  numeric config; see [extensions.md](extensions.md) §2.
- Moving store selection (in-memory/Postgres/Redis) into `.cstack` — backend
  selection is a macro-invocation/app-wiring concern today (mirroring `db =
  Postgres` on `include_server_schema!`, which lives outside the `.cstack`
  body), and stays that way.
- Moving TTL, burst, refill rate, or key/fingerprint functions into schema
  syntax even for the deferred idempotency flag — only the participation
  boolean is in scope, never the tuning.
