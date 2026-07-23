# Extensions — a declarative surface for opt-in framework capabilities

Status: **in progress** (2026-07-23) — design accepted, implementation
under way. Tickets #153 (grammar) and #161 (Cargo-feature enforcement) are
shipped (see §8); #154/#155/#156/#157 remain open. This document
supersedes the rate-limiting half of the decision recorded in
[idempotency-rate-limit-declarative-surface.md][prior-doc] and folds it into
a new, more general concept. Revised twice since the original proposal:
once to make explicit that an extension is declared in three separate
layers (§2), and again after #161's implementation found the originally
proposed enforcement mechanism (`CARGO_FEATURE_<NAME>` env vars) doesn't
actually work inside a proc-macro — see §2's "Enforcement mechanism
(revised after implementation)" note for what was built instead.
Scope: `cratestack-parser` grammar, `cratestack-macros` codegen, Cargo
feature surface of `cratestack-migrate`/`cratestack-sqlx`/`cratestack-pg`/
`cratestack-axum`, DDL emission, rate-limit wiring.
Tracking: [#139][139] (original spike; this doc reframes its scope — see §9).

[prior-doc]: idempotency-rate-limit-declarative-surface.md
[139]: https://github.com/cratestack/cratestack/issues/139

## Summary

| Item | Decision |
|---|---|
| New top-level `.cstack` construct | `extension <name> { ... }` — a schema-level block, sibling to `model`/`mixin`/`mcp`, not a model attribute. |
| Default-supported extensions | `rate_limit` and `pgvector`. Nothing else in scope for this doc. |
| What `.cstack` declares | **Participation/capability only** — that this schema uses the capability, and (for `rate_limit`) which operations opt out. Never numeric tuning (limits, TTLs, CA certs, distance-metric parameters) — that stays imperative/env-driven, unchanged from today. |
| What gates the actual code | **A same-named Cargo feature** on every crate whose codegen/runtime needs to change — not the `.cstack` declaration alone. Declaring `extension pgvector { }` without the `pgvector` feature enabled on the relevant crate is a compile error, not a silent no-op. See §2. |
| What stays swappable | **The implementation.** Each extension defines a trait boundary; the framework ships a reference implementation, but nothing about the extension mechanism requires using it — same freedom already exercised by `AuditSink`/`RateLimitStore`/`IdempotencyStore` today. See §2. |
| Relationship to the closed rate-limit decision | Reframed, not silently reversed — see §3. The prior doc's objection was to compiling *tunable numbers* into `.cstack`; this proposal never does that. |
| Implementation status | None. This doc only restructures #139 into a design + scoped follow-up tickets (§8). |

## 1. The idea

Three things already sit on the declarative `.cstack` surface: model-level
booleans (`@@audit`, `@@soft_delete`, `@@paged`), scalar field types (`Cuid`,
`Decimal`, `Uuid`, ...), and top-level blocks (`model`, `mixin`, `type`,
`enum`, `datasource`, `mcp`, plus the bare `transport` directive). All of
them describe the schema's own shape or a fact about how a specific
declared thing behaves — never *deployment-varying operational policy*.

"Extensions" is a fourth, orthogonal thing: **an opt-in framework or database
capability that the schema wants to use**, declared once at the top level,
that other declarations (scalar types, attributes, index kinds) become
available or meaningful in light of. This is exactly what "extension" means
in Postgres itself — `CREATE EXTENSION vector` doesn't configure anything,
it makes a capability (a type, some operators, an index access method)
available for everything declared afterward. The proposal borrows that
framing directly: `extension pgvector { }` in `.cstack` is the schema-level
announcement that mirrors `CREATE EXTENSION vector` in the database it
targets, and unlocks the `Vector(n)` scalar type plus (later) vector index
kinds for use in `model` blocks in the same file.

`rate_limit` doesn't need a database extension counterpart, but the same
shape fits: `extension rate_limit { }` announces that this schema's
generated dispatch layer expects rate limiting to be wired up, and gives
per-procedure declarations (`@no_rate_limit`) a place to live — mirroring
exactly how `@no_idempotency` was already sketched as a procedure attribute
in the prior doc's §5, just generalized to be gated on the extension being
declared at all.

## 2. Three separate layers — declare, gate, implement

An earlier revision of this doc treated "declared in `.cstack`" as the
whole mechanism. That's incomplete: it would mean every consumer of, say,
`cratestack-migrate` pays for pgvector's DDL/type-mapping code whether or
not any schema they compile ever declares the extension, and it would mean
"is this extension available" and "does it compile" are the same question
— they aren't. The corrected model has three independent layers:

1. **`.cstack` declaration** (schema-level, per §4/§5 below) — a
   schema-visible statement of intent. This is what unlocks *syntax*:
   `Vector(n)` as a valid field type, `@no_rate_limit` as a valid procedure
   attribute. It is parsed and validated the same regardless of what the
   consuming crate was compiled with.
2. **Cargo feature gate** (crate-level, one per extension, same name as
   the `.cstack` extension) — this is what makes the *code* exist at all.
   `cratestack-migrate`'s pgvector DDL emission, `cratestack-sqlx`'s
   `vector(n)` column support, and `cratestack-axum`'s rate-limit dispatch
   plumbing each live behind a Cargo feature (`pgvector`, `rate_limit`)
   that a consuming crate opts into explicitly — directly following the
   precedent just shipped for Redis TLS support (`tls-rustls` as an
   optional feature on `cratestack-redis`, gating real dependencies and
   code paths that non-TLS users never pay for). If a schema's `.cstack`
   declares `extension pgvector { }` but the crate compiling it wasn't
   built with the `pgvector` feature, the generating macro must fail the
   build with a clear `compile_error!`.

   **Enforcement mechanism (revised after implementation — see #161):**
   the original version of this doc proposed reading `CARGO_FEATURE_<NAME>`
   environment variables at proc-macro expansion time, on the assumption
   this worked like the `decimal-*` mutual-exclusion check in
   `cratestack-core`. Verified empirically while building #161 (a
   standalone probe proc-macro crate dumping every `CARGO*` env var it
   could see) and found to be **wrong**: `CARGO_FEATURE_<NAME>` is
   build-script-only — a proc-macro expands inside the `rustc` process
   compiling the invoking crate, which never receives those variables.
   The `decimal-*` precedent doesn't use env vars either; it's a plain
   `#[cfg(feature = "...")]`/`compile_error!` pair evaluated by rustc
   while compiling `cratestack-core` *against its own* features — not
   transferable as-is to a proc-macro crate checking a different,
   downstream crate's features.

   **What actually works:** `cratestack-macros` declares the same-named
   features (`rate_limit`, `pgvector`) itself, and the check uses
   `cfg!(feature = "...")` evaluated against `cratestack-macros`' *own*
   compiled-in feature set — not the invoking crate's. This works because
   of Cargo's standard feature-forwarding/unification: a facade crate
   (`cratestack-pg`, `cratestack-sqlite`) forwards its own `pgvector`
   feature down to `cratestack-macros` via `pgvector =
   ["cratestack-macros/pgvector"]` in its `Cargo.toml`, so enabling the
   feature on the facade the app actually depends on transitively enables
   it on `cratestack-macros` too — the same technique `sqlx`/`sqlx-macros`
   use for exactly this kind of macro-visible feature gate. Confirmed
   end-to-end in #161 with a throwaway scratch crate: a schema declaring
   `extension pgvector {}` produced a real `compile_error!` by default,
   and enabling the feature *on the `cratestack-macros` dependency edge*
   turned it off, proving the forwarding chain actually works.
3. **Implementation** (trait-level, swappable) — the extension's actual
   behavior is never hardcoded to one implementation. `rate_limit` already
   has this shape today (`Arc<dyn RateLimitStore>`, three interchangeable
   backends); `pgvector` gets the same treatment as its trait surface is
   designed in the follow-up tickets (§8) — e.g. nothing in this proposal
   requires using pgvector's own index types if a user wants to plug in a
   different similarity-search backend behind the same generated column
   shape. The framework ships one reference implementation per extension;
   using it is the default, not the only option, exactly mirroring how
   `AuditSink` today has `NoopAuditSink`/`MulticastAuditSink` but nothing
   stops a third implementation.

**Consequence:** turning `rate_limit` into a first-class extension also
means moving its *existing*, always-compiled code in `cratestack-axum`
behind a new `rate_limit` feature, consistent with layer 2 above. That is
a breaking change for any current consumer linking `cratestack-axum` and
using `RateLimitLayer` without any feature flag today — call this out
explicitly for whoever reviews this doc rather than deciding it silently;
per this repo's own delivery-style convention (hard cutovers, no
default-on backward-compat shims kept "just in case"), the expectation is
that the follow-up ticket ships the feature gate live and non-default, not
soft-launches it behind a default-enabled flag.

## 3. Why this doesn't just re-litigate the closed decision

[idempotency-rate-limit-declarative-surface.md §4.1][prior-doc] closed
`@@rate_limit(burst: 100, refill: 10.0)`-style attributes permanently,
for two reasons: (a) a numeric limit is deployment traffic budget, not
model shape, and (b) `.cstack` compiles to `pub const`s, so retuning a
limit during an incident would mean recompile + redeploy instead of an
env-var flip.

Both objections are about **carrying the tunable numbers themselves** into
the schema. This proposal doesn't do that, anywhere. `extension rate_limit
{ }` carries zero numeric configuration — no burst, no refill rate, no
window. `RateLimitConfig` stays exactly what it is today: constructed at
app startup from env/config, unchanged (only *where its code lives* changes
per §2's Cargo-feature consequence — not how it's configured). What the
extension block adds is narrower and mirrors the *existing* declarative
attributes' own discipline (§2 of the prior doc: "the schema only declares
the shape; wiring stays imperative even for the concerns that are
otherwise declarative"):

- Whether the generated dispatch layer should assume rate limiting exists
  at all (today this is 100% assembled by hand in the consuming app; there
  is no schema-visible signal either way).
- Which procedures opt out (`@no_rate_limit`), so the schema — not just an
  app-level middleware config — is the source of truth for "is this
  endpoint rate-limited," the same way `@@soft_delete` is the source of
  truth for "does this model have a `deleted_at` column," not a runtime
  flag guessed at by callers.
- For generated clients: whether to emit 429-aware retry/backoff behavior
  by default, since the client can now see the capability is declared.

If a future need genuinely requires deployment-varying *numbers* in the
schema, that remains closed per the prior doc — this proposal does not
reopen it. What's reframed is narrower: participation becomes declarative;
tuning stays imperative, permanently, exactly as decided.

## 4. Grammar

Per research into `crates/cratestack-parser/src/parse/mod.rs:45-193`, the
top-level grammar is a hand-rolled dispatch over line prefixes (not a
chumsky-parsed enum) — `datasource`, `auth`, `mixin`, `model`, `type`,
`enum` route through `parse_body_block`/`parse_named_config_block`; `mcp {`
through `parse_simple_config_block`; bare `transport rpc`/`transport rest`
through a directive parser with no braces at all
(`crates/cratestack-parser/src/parse/blocks.rs:6`).

`extension <name> { ... }` is additive to this list — the same shape as
`mcp { }` (a simple top-level config block keyed by name), not a novel
grammar construct:

```cstack
extension rate_limit {
}

extension pgvector {
}

model Document {
  id        Cuid    @id
  embedding Vector(1536)
}

procedure createPayment(...) @no_rate_limit {
  ...
}
```

An extension block with no recognized body content (as both defaults are
in phase 1 — see §8) is valid and simply toggles the capability on; body
content is reserved for later per-extension config that is itself
non-tunable (e.g. `pgvector`'s block could later carry `schema: "public"`
placement, never distance-metric weights or index-build parameters, which
stay imperative for the same reason rate-limit numbers do).

Declaring an unknown extension name is a parse error, same as an unknown
top-level keyword today (`mod.rs:188-192`). Declaring a *known* extension
name whose matching Cargo feature isn't enabled on the compiling crate is
a distinct, later error — a compile error raised by the macro at expansion
time, per §2 layer 2 — not a parse error, since parsing the `.cstack` file
doesn't know what Cargo features the consuming crate has.

## 5. Extension: `rate_limit`

- **`.cstack` declares:** that the generated dispatch layer participates in
  rate limiting, and that `@no_rate_limit` is a valid procedure attribute
  in this schema.
- **Cargo feature:** `rate_limit`, declared on `cratestack-macros` itself
  (the enforcement check in #161 evaluates `cfg!(feature = "rate_limit")`
  against `cratestack-macros`' own compiled features — see §2's revised
  mechanism), forwarded down from `cratestack-axum` and the
  `cratestack-pg`/`cratestack-sqlite` facades via `rate_limit =
  ["cratestack-macros/rate_limit"]`. Gates the dispatch-layer codegen that
  reads `rate_limited_by_default`; per §2, this also means
  `RateLimitLayer`'s existing code moves behind this feature rather than
  staying always-compiled.
- **Does not declare:** burst, refill rate, window, key/fingerprint
  function, or store backend (in-memory/Postgres/Redis) — all of that
  stays exactly where it is today, constructed imperatively at app
  startup (`crates/cratestack-axum/src/ratelimit/{layer,config,store}.rs`).
- **Implementation stays swappable:** `Arc<dyn RateLimitStore>` already has
  three interchangeable backends; nothing here changes that trait boundary
  or requires a specific one.
- **Codegen surface:** a `rate_limited_by_default: bool` (or per-procedure
  descriptor field) analogous to `OpDescriptor.idempotent_by_default`
  from the RPC transport design — read by the dispatcher, not baked into
  a limit value.

## 6. Extension: `pgvector`

- **`.cstack` declares:** that this schema uses Postgres's `vector`
  extension. Unlocks the `Vector(n)` scalar field type (fixed-dimension
  float vector, `n` a compile-time literal) for use in `model` blocks.
- **Cargo feature:** `pgvector`, declared on `cratestack-macros` itself
  (same mechanism as `rate_limit` above — see §2's revised mechanism),
  forwarded down from `cratestack-migrate` (DDL emission) and
  `cratestack-sqlx`/`cratestack-pg` (column type support, query-builder
  hooks once those phases land) via `pgvector =
  ["cratestack-macros/pgvector"]`. A schema declaring `extension pgvector {
  }` compiled against a crate without this feature enabled anywhere along
  the forwarding chain is a compile error per §2. Note: for
  `include_embedded_schema!` specifically, `pgvector` is rejected
  unconditionally regardless of any feature — it's inherently a Postgres
  extension, so no Cargo feature could make it valid against the
  rusqlite-only embedded backend; #161 gives this its own clearer error
  rather than a generic "feature not enabled" message.
- **DDL surface (phase 1 — see §8):** `cratestack-migrate` emits `CREATE
  EXTENSION IF NOT EXISTS vector;` once per schema that declares the
  extension, before any DDL referencing a `Vector(n)` column — currently
  no code path emits `CREATE EXTENSION` anywhere
  (the only existing reference is a manual test fixture,
  `crates/cratestack-pg/tests/banking_builder_extensions_tier7.rs:30`, not
  generated). Column DDL maps `Vector(n)` → Postgres `vector(n)` in
  `crates/cratestack-migrate/src/emit/postgres/columns.rs:150-167`
  (`scalar_to_postgres`), which today silently falls through unknown
  scalars to `TEXT` — `Vector(n)` needs to be a recognized, parametric
  case, not a silent fallback.
- **Index DDL (phase 2 — deferred, see §8):** `ivfflat`/`hnsw` index
  support needs `AddIndex` (`crates/cratestack-migrate/src/ir/ops.rs:33-37`)
  extended with a `using`/`method`/`opclass` field — there is currently no
  `@@index(...)` attribute at all (only `@unique`,
  `crates/cratestack-migrate/src/convert/fields.rs:48-52`), so this phase
  also needs a new attribute, not just a new index kind.
- **Implementation stays swappable (from phase 2 onward):** the DDL/type
  shape (`vector(n)` column, `CREATE EXTENSION`) is the framework's fixed
  contract, but the index/distance-metric strategy should land behind a
  trait boundary rather than hardcoding pgvector's own index types as the
  only option — the follow-up tickets should design phase 2/3 with this in
  mind from the start rather than retrofitting it later.
- **Query builder / client codegen:** distance operators (`<->`, `<=>`,
  `<#>`) and similarity-search query methods are explicitly out of scope
  for phase 1 — see §7.

## 7. Explicitly not proposed here

- Any numeric or environment-tuned configuration inside an `extension { }`
  block, for either default extension. Reopening that is a separate,
  explicit decision — not a side effect of this doc.
- `pgvector` index DDL (ivfflat/hnsw), the `@@index(...)` attribute
  generally, or query-builder distance-operator support. Real scope,
  deferred to its own ticket (§8) rather than bundled in sight-unseen.
- Client codegen (Rust/TS/Dart) for `Vector(n)` fields or similarity
  search. Same reason.
- A generic plugin/extensibility SDK for third-party extensions beyond
  the two named here. `extension <name>` recognizes a closed, framework-
  maintained list (`rate_limit`, `pgvector`) — not an arbitrary-extension
  mechanism. If a third-party-extension story is wanted later, that's a
  new design question, not an implicit consequence of this one.
- Any change to the idempotency finding in the prior doc (§4.2 there is
  untouched — idempotency stays gated on `OpExecutor`, unrelated to this).
- Deciding, in this doc, whether the `rate_limit` Cargo feature ships
  default-on (soft transition) or default-off (hard cutover) — flagged as
  a call for the follow-up ticket's reviewer in §2, not resolved here.

## 8. Follow-up tickets (design only — nothing here is scoped for implementation)

Sequenced; each is independently shippable and should be opened as its own
Dev Ticket once this doc is accepted, linked under the Epic in §9:

1. **Extensions grammar** (Feature) — **shipped**, `feat/153-extensions-grammar`
   (#153). `extension <name> { }` top-level parsing in `cratestack-parser`,
   recognizing exactly `rate_limit` and `pgvector` as valid names (unknown
   name = parse error), threading a `declared_extensions:
   BTreeSet<ExtensionKind>` onto the parsed `Schema`. No behavior change in
   any backend — parse-and-record only.
2. **Cargo feature enforcement mechanism** (Feature, depends on #1) —
   **shipped**, `feat/161-extension-feature-enforcement` (#161). Turns
   "schema declares an extension the crate wasn't built with" into a
   `compile_error!` in all three entry macros, per §2 layer 2's revised
   mechanism (features declared on `cratestack-macros` itself, forwarded
   from facade crates — not `CARGO_FEATURE_<NAME>`, which doesn't work
   inside a proc-macro; see §2). Built once, reused by every extension
   rather than each one reimplementing its own check.
3. **`rate_limit` extension wiring** (Feature, depends on #2) — gate
   `cratestack-axum`'s existing `RateLimitLayer`/`RateLimitConfig`/store
   code behind a new `rate_limit` Cargo feature (default-on vs default-off
   is this ticket's call to make, informed by §2's breaking-change note);
   consume the parsed extension in `cratestack-macros`; add
   `@no_rate_limit` as a valid procedure attribute (parser + codegen);
   thread a `rate_limited_by_default` field onto generated procedure
   descriptors.
4. **`pgvector` phase 1: DDL + scalar type** (Feature, depends on #2) —
   `Vector(n)` scalar recognition in `type_names.rs`/`shared/types.rs`
   behind a new `pgvector` Cargo feature on `cratestack-migrate` and
   `cratestack-sqlx`/`cratestack-pg`; `CREATE EXTENSION IF NOT EXISTS
   vector;` emission in `cratestack-migrate` gated on the schema declaring
   the extension; column DDL mapping to `vector(n)`.
5. **`pgvector` phase 2: index DDL** (Feature, depends on #4) — `@@index`
   attribute (currently doesn't exist at all) generalized enough to carry
   `using: ivfflat` / `using: hnsw` + `opclass`, `AddIndex` IR extended
   accordingly, with the index/distance-metric strategy behind a trait
   boundary per §6's "implementation stays swappable" note.
6. **`pgvector` client/query-builder support** (Spike first) — whether
   distance-operator query-builder methods and client codegen are worth
   the surface area before committing to a shape; spike per this repo's
   own convention of scoping speculative surface as a spike before a
   Feature ticket.

## 9. Relationship to #139 and the prior doc

[#139][139] is left open as the historical record of the original spike
and its split decision (PR #146, merged 2026-07-22) — that decision is not
being erased, only the rate-limit half is being reframed per §3. A new
Epic issue should be opened for "Extensions" that references #139 as prior
art and links this document as its source of truth, with the tickets in §8
opened underneath it once the Epic is accepted. `docs/design/idempotency-rate-limit-declarative-surface.md`'s status line
should be updated to point here rather than silently going stale.

## 10. Non-goals

- Reopening deployment-tunable numbers in `.cstack` for rate limiting —
  still closed, see §3.
- A general third-party extension SDK — see §7.
- Any idempotency changes — untouched, still gated on `OpExecutor` per the
  prior doc's §4.2/§5/§6.
- Hardcoding pgvector's own index types as the only supported similarity-
  search backend — the implementation stays swappable per §2/§6.
