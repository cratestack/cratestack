# Extensions — a declarative surface for opt-in framework capabilities

Status: **proposed** (2026-07-23) — design only, no code shipped. This
document supersedes the rate-limiting half of the decision recorded in
[idempotency-rate-limit-declarative-surface.md][prior-doc] and folds it into
a new, more general concept. It does not implement anything; §7 scopes the
follow-up tickets.
Scope: `cratestack-parser` grammar, `cratestack-macros` codegen,
`cratestack-migrate` DDL emission, `cratestack-axum` rate-limit wiring.
Tracking: [#139][139] (original spike; this doc reframes its scope — see §8).

[prior-doc]: idempotency-rate-limit-declarative-surface.md
[139]: https://github.com/cratestack/cratestack/issues/139

## Summary

| Item | Decision |
|---|---|
| New top-level `.cstack` construct | `extension <name> { ... }` — a schema-level block, sibling to `model`/`mixin`/`mcp`, not a model attribute. |
| Default-supported extensions | `rate_limit` and `pgvector`. Nothing else in scope for this doc. |
| What an extension declares | **Participation/capability only** — that this schema uses the capability, and (for `rate_limit`) which operations opt out. Never numeric tuning (limits, TTLs, CA certs, distance-metric parameters) — that stays imperative/env-driven, unchanged from today. |
| Relationship to the closed rate-limit decision | Reframed, not silently reversed — see §2. The prior doc's objection was to compiling *tunable numbers* into `.cstack`; this proposal never does that. |
| Implementation status | None. This doc only restructures #139 into a design + scoped follow-up tickets (§7). |

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

## 2. Why this doesn't just re-litigate the closed decision

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
app startup from env/config, unchanged. What the extension block adds is
narrower and mirrors the *existing* declarative attributes' own discipline
(§2 of the prior doc: "the schema only declares the shape; wiring stays
imperative even for the concerns that are otherwise declarative"):

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

## 3. Grammar

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
in phase 1 — see §7) is valid and simply toggles the capability on; body
content is reserved for later per-extension config that is itself
non-tunable (e.g. `pgvector`'s block could later carry `schema: "public"`
placement, never distance-metric weights or index-build parameters, which
stay imperative for the same reason rate-limit numbers do).

Declaring an unknown extension name is a parse error, same as an unknown
top-level keyword today (`mod.rs:188-192`).

## 4. Extension: `rate_limit`

- **Declares:** that the generated dispatch layer participates in rate
  limiting, and that `@no_rate_limit` is a valid procedure attribute in
  this schema.
- **Does not declare:** burst, refill rate, window, key/fingerprint
  function, or store backend (in-memory/Postgres/Redis) — all of that
  stays exactly where it is today, constructed imperatively at app
  startup (`crates/cratestack-axum/src/ratelimit/{layer,config,store}.rs`).
- **Codegen surface:** a `rate_limited_by_default: bool` (or per-procedure
  descriptor field) analogous to `OpDescriptor.idempotent_by_default`
  from the RPC transport design — read by the dispatcher, not baked into
  a limit value.

## 5. Extension: `pgvector`

- **Declares:** that this schema uses Postgres's `vector` extension.
  Unlocks the `Vector(n)` scalar field type (fixed-dimension float vector,
  `n` a compile-time literal) for use in `model` blocks.
- **DDL surface (phase 1 — see §7):** `cratestack-migrate` emits `CREATE
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
- **Index DDL (phase 2 — deferred, see §7):** `ivfflat`/`hnsw` index
  support needs `AddIndex` (`crates/cratestack-migrate/src/ir/ops.rs:33-37`)
  extended with a `using`/`method`/`opclass` field — there is currently no
  `@@index(...)` attribute at all (only `@unique`,
  `crates/cratestack-migrate/src/convert/fields.rs:48-52`), so this phase
  also needs a new attribute, not just a new index kind.
- **Query builder / client codegen:** distance operators (`<->`, `<=>`,
  `<#>`) and similarity-search query methods are explicitly out of scope
  for phase 1 — see §6.

## 6. Explicitly not proposed here

- Any numeric or environment-tuned configuration inside an `extension { }`
  block, for either default extension. Reopening that is a separate,
  explicit decision — not a side effect of this doc.
- `pgvector` index DDL (ivfflat/hnsw), the `@@index(...)` attribute
  generally, or query-builder distance-operator support. Real scope,
  deferred to its own ticket (§7) rather than bundled in sight-unseen.
- Client codegen (Rust/TS/Dart) for `Vector(n)` fields or similarity
  search. Same reason.
- A generic plugin/extensibility SDK for third-party extensions beyond
  the two named here. `extension <name>` recognizes a closed, framework-
  maintained list (`rate_limit`, `pgvector`) — not an arbitrary-extension
  mechanism. If a third-party-extension story is wanted later, that's a
  new design question, not an implicit consequence of this one.
- Any change to the idempotency finding in the prior doc (§4.2 there is
  untouched — idempotency stays gated on `OpExecutor`, unrelated to this).

## 7. Follow-up tickets (design only — nothing here is scoped for implementation)

Sequenced; each is independently shippable and should be opened as its own
Dev Ticket once this doc is accepted, linked under the Epic in §8:

1. **Extensions grammar** (Feature) — `extension <name> { }` top-level
   parsing in `cratestack-parser`, recognizing exactly `rate_limit` and
   `pgvector` as valid names (unknown name = parse error), threading a
   `declared_extensions: BTreeSet<ExtensionKind>` onto the parsed
   `Schema`. No behavior change in any backend yet — parse-and-record only.
2. **`rate_limit` extension wiring** (Feature) — consume the parsed
   extension in `cratestack-macros`; add `@no_rate_limit` as a valid
   procedure attribute (parser + codegen), thread a
   `rate_limited_by_default` field onto generated procedure descriptors.
   No change to `RateLimitLayer`/`RateLimitConfig` construction.
3. **`pgvector` phase 1: DDL + scalar type** (Feature) — `Vector(n)` scalar
   recognition in `type_names.rs`/`shared/types.rs`, `CREATE EXTENSION IF
   NOT EXISTS vector;` emission in `cratestack-migrate` gated on the
   schema declaring the extension, column DDL mapping to `vector(n)`.
4. **`pgvector` phase 2: index DDL** (Feature, depends on #3) — `@@index`
   attribute (currently doesn't exist at all) generalized enough to carry
   `using: ivfflat` / `using: hnsw` + `opclass`, `AddIndex` IR extended
   accordingly.
5. **`pgvector` client/query-builder support** (Spike first) — whether
   distance-operator query-builder methods and client codegen are worth
   the surface area before committing to a shape; spike per this repo's
   own convention of scoping speculative surface as a spike before a
   Feature ticket.

## 8. Relationship to #139 and the prior doc

[#139][139] is left open as the historical record of the original spike
and its split decision (PR #146, merged 2026-07-22) — that decision is not
being erased, only the rate-limit half is being reframed per §2. A new
Epic issue should be opened for "Extensions" that references #139 as prior
art and links this document as its source of truth, with the five tickets
in §7 opened underneath it once the Epic is accepted. `docs/design/idempotency-rate-limit-declarative-surface.md`'s status line
should be updated to point here rather than silently going stale.

## 9. Non-goals

- Reopening deployment-tunable numbers in `.cstack` for rate limiting —
  still closed, see §2.
- A general third-party extension SDK — see §6.
- Any idempotency changes — untouched, still gated on `OpExecutor` per the
  prior doc's §4.2/§5/§6.
