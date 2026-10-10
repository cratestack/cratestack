# CrateStack Roadmap

**Last updated: 2026-09-24 · current release: 0.12.0**

This is a map of where CrateStack is, what's being built, and what's been
deliberately ruled out. It is not a schedule and not a commitment. CrateStack is
pre-1.0 and moves fast — 0.2.0 shipped on 2026-05-12 and 0.12.0 on 2026-09-06,
so anything below marked "considering" may land next month or never.

Three things this document tries to do that a changelog can't:

1. Say what's **shipped** in one place, because the changelog is 7,000 lines.
2. Say what's **being worked on right now**, and what's unclaimed.
3. Say what we've **decided against**, and why — so the same question doesn't
   get re-litigated every quarter.

Items marked 🔵 **needs a decision** are open maintainer calls. They are listed
rather than resolved on purpose.

---

## Where CrateStack is today

### The core is done and in use

| Capability | Since | Notes |
| --- | --- | --- |
| `.cstack` parser + semantic checker | 0.1 | chumsky-based, multi-error reporting |
| Three entry macros, strictly disjoint | 0.3.0 | server / embedded / client — CI-enforced disjointness |
| Four facades (`-pg`, `-api`, `-sqlite`, `-client`) | 0.4.0 | selected via Cargo `package =` rename |
| Postgres backend (sqlx) + Axum routes | 0.1 | CRUD, procedures, projections, filters, relations |
| Row-level policy (`@@allow` / `@@deny`) | 0.1 | compiled into the SQL of every read and write |
| Embedded SQLite (rusqlite), native + `wasm32` | 0.3.0 | OPFS persistence in the browser; Flutter & React Native (Expo) mobile hosts; sync API |
| Generated clients: Rust, Dart, TypeScript | 0.2–0.6 | same projection contract across all three |
| CBOR + JSON codecs | 0.1 | CBOR is the default wire format |
| RPC transport (`POST /rpc/{op_id}`, `/rpc/batch`) | 0.5–0.6 | mutually exclusive with REST per schema |
| RPC streaming (incremental delivery) | 0.6.0 | plus a first-party CBOR codec family |
| Migrations: diff, FKs, `onDelete`/`onUpdate`, unique indexes | 0.6.0 | `cratestack migrate` |
| **Live-database introspection + `migrate baseline`** | 0.6 era | adopt CrateStack against an existing populated database |
| SQL views, incl. `@@materialized` + `refresh()` | 0.9 era | [ADR-0003](https://cratestack.dev/internals/views-adr) |
| `db = None` — procedures-only, no database | 0.7 era | `sqlx` genuinely absent from the graph |
| Computed fields (`@computed`) | shipped | resolver-backed, response-time |
| Extensions — declarative opt-in capabilities | 0.7 era | the plugin surface; `docs/design/extensions.md` |
| Decimal backends, additive | 0.8.0 | `rust_decimal` **and** `bigdecimal` in one build |
| Idempotency + rate limiting (Redis / SQL / in-memory) | 0.7–0.11 | typed error bodies as of 0.11.0 |
| Transactional outbox | shipped | `cratestack-outbox` |
| Auth: Ed25519 request signing, SD-JWT, multi-issuer JWKS | shipped | `cratestack-auth` |
| PostGIS + pgvector column types | 0.10.0 | declarable in schema |
| Declarative parameterized custom SQL (`query` blocks) | 0.11.0 | the escape hatch, schema-authored |
| Studio (admin/testing UI) + `studio eject` | 0.3–0.9 | Leptos+Trunk, served from `studio.toml` |
| LSP + VS Code extension (Marketplace + Open VSX) | 0.10.1 | `.cstack` language server |
| WireMock stub generation | shipped | mock backends that can't drift from the schema |
| `@cratestack/*` npm family (15 packages) | 0.6.0+ | fetch/axios runtimes, TanStack, RTK, Refine, zod/yup, CBOR |

### The engineering discipline is also shipped

Worth stating, because it's unusual for a pre-1.0 project and it's what makes
the above trustworthy: `unsafe_code = "forbid"` workspace-wide with a CI
opt-in guard, a CI-enforced layer-direction rule ([ADR 0014](docs/adr/0014-layer-direction-enforcement.md)),
a facade-disjointness job that proves absence with real `cargo tree` runs, a
200-line file ceiling with a dated allowlist, and a changelog placement check.

---

## In flight

### 🚧 OpExecutor — the L3 execution layer ([#875](https://github.com/cratestack/cratestack/issues/875))

The architectural epic in progress. `docs/design/rpc-transport.md` §4 specified an
`OpExecutor` on 2026-05-15 and `docs/design/layering.md` §2 called L3 "the one
layer with no members" for three months. [ADR 0015](docs/adr/0015-op-executor-l3.md)
(accepted, amended 2026-09-03) settles building it in slices.

The problem it solves is precise: idempotency, rate limiting, row-level policy
and audit fan-out are applied at **four different layers**, so there is no point
at which an operation can be seen whole — see `docs/design/layering.md` §5.1 for
the table. `ADR 0018` made in-process invocation a compatibility-committed
dispatch path, which is the second input shape that made building L3 correct.

| Slice | Scope | Status |
| --- | --- | --- |
| 1 | Idempotency admission → L3; `@no_idempotency` goes live | ✅ landed ([#876](https://github.com/cratestack/cratestack/issues/876)) |
| 2 | Rate-limit admission → L3; `@no_rate_limit` honoured under `Router::nest` | ✅ landed ([#877](https://github.com/cratestack/cratestack/issues/877), [#995](https://github.com/cratestack/cratestack/pull/995)) |
| 3 | Row-level policy replayed against streamed `ModelEvent<T>` | planned |
| 4 | Audit fan-out | planned |

Slice 3 closes a real correctness gap, not just a tidiness one: a subscriber on
the SSE path authenticates but currently gets **no per-row filtering**, because
`@@allow` is compiled into SQL and streamed events never pass through it.

### 🆕 React Native (Expo + bare) as a client target ([#893](https://github.com/cratestack/cratestack/issues/893))

React Native is the one major mobile host with no client SDK coverage
(`examples/embedded-expo` covers the embedded SQLite host shape, while client
generation remains in progress). Scoped into five stories: a Rust-backed CBOR
codec that runs on device, a `react-native` export condition across the shared
packages, a generated RN client following the Dart preset pattern, a generated
RTK Query endpoint set, and Expo + bare examples.

Start at [#899](https://github.com/cratestack/cratestack/issues/899), a decision
ticket: Hermes is gaining WebAssembly support, and if that lands broadly
`@cratestack/cbor-web` may serve React Native with no native module at all.

### 🆕 Multi-file schemas — `part` / `import` ([#910](https://github.com/cratestack/cratestack/issues/910))

Split a large `.cstack` across thematic files (`part` / `part of`), and reuse a
shared schema across services (`import`). Blocked at the front on
[#915](https://github.com/cratestack/cratestack/issues/915), which settles
`import` semantics before any parser work — including one product decision
(whether imported `model`s create tables) reserved for the maintainer.

### 🆕 `Int` and `BigInt` as built-in integer types: the breaking 0.16.0 release ([#1130](https://github.com/cratestack/cratestack/pull/1130))

Accepted on 2026-09-30 as [ADR 0019](docs/adr/0019-int-and-bigint-built-in-types.md),
with the per-target changes, diagrams and the three-PR plan in
[`docs/design/int-and-bigint.md`](docs/design/int-and-bigint.md) (§7). The ADR
landed in #1130. **Handoff, 2026-10-09:** PR A and PR B are built and land on
`main` together; PR C (the `Int` cutover) and the live-server TypeScript round
trip B13 are not built and pass to a colleague. Landing A and B is not a
release: B and C ship together in 0.16.0, so no release is cut from `main`
until C lands. The ADR's "Amendment (PR B, 2026-10-09)" records where the build
differs from the plan.

The problem is precise: a schema `Int` is a Rust `i64` and a JSON number that
carries every digit, and no JavaScript consumer can hold that range. TypeScript
rounds anything above 2^53 without an error, `@cratestack/cbor-web` refuses to
decode it, and Dart on dart2js rounds it. skyport-billing, which stores money as
`i64` e8 units, is the reported victim. There is no per-field fix today: an
attribute such as `@string` reports `schema OK` and changes nothing.

What the ADR decides:

- **`Int` becomes `i32`** (`INTEGER`), exact in every client by construction.
- **`BigInt` is the new built-in `i64`**: `cratestack::BigInt` in Rust, `bigint`
  in TypeScript, `BigInt` in Dart. It travels as a canonical decimal string on
  JSON **and** on CBOR (a text string, major type 3), so there is one wire form
  on every codec, and a JSON number or a CBOR integer at a `BigInt` key is
  refused.
- **Field attributes become a closed list** per declaration kind, so an unknown
  name is a `check` error instead of a silent no-op.
- **`cratestack upgrade int-to-bigint`** rewrites every `Int` to `BigInt` by
  parser span and moves snapshots to format 3, with no DDL. Narrowing a field
  back to `Int` is a deliberate per-field edit, `Lossy` in `migrate diff`. The
  command ships in 0.16.0 and is deleted in 0.17.0.

All three PRs ship together in **0.16.0**, one breaking release with no
compatibility path and no release cut between them. A user regenerates every
client and rebuilds every server in one upgrade.

| PR | Scope | Status |
| --- | --- | --- |
| A | Field attributes become a closed list per declaration kind ([#679](https://github.com/cratestack/cratestack/issues/679) option (a)); validators on a `type` field and on a `model` procedure argument are enforced ([#1156](https://github.com/cratestack/cratestack/issues/1156), fixed by this merge) | **done, merged** (2026-10-09), unreleased |
| B | `BigInt` end to end: core type and serde on both codecs, SQL, migrate, JSON Schema, MCP, TypeScript and Dart clients, fail-closed policies | **done, merged** (2026-10-09), unreleased. Tests: `cratestack-pg` `bigint_end_to_end`, `bigint_end_to_end_rpc`, `bigint_policies`, `bigint_wire_shapes`; `cratestack-sqlite` `bigint_round_trip` |
| B13 | The generated TypeScript client driven against a real server (JSON and `@cratestack/cbor-node`, `@cratestack/link-batch`, a REST body and query, TanStack and RTK keys) | **pending** (colleague). Until then the TypeScript claim rests on the client-side suites in `crates/cratestack-client-typescript/tests/js` |
| C | `Int` becomes 32-bit: snapshot format 3, digest domains `v2`, the `int-to-bigint` codemod | **pending** (colleague); starts from design §7, "What C inherits from B" |
| Docs and skills | The `cratestack-docs` pages and `cratestack-skills` entries named in design §4.4 | **pending** |

B and C must ship together, or a hand edit from `Int` to `BigInt` would be a
`Lossy` `ALTER` to the same type. A is technically independent and rides the
same release. Until C lands, `Int` is still an `i64`, and the `## Unreleased`
entry for B says so.

#### Follow-ups for the colleague

Found while building A and B. Each is verified or marked as a decision; none is
fixed by the merge unless it says so.

Policies (`!=`, `not in` and `@deny` now fail closed for `BigInt`; these are the
edges that remain):

- **String-claim coercion in policies.** A string auth claim in a numeric
  comparison denies today. Accepting a canonical `BigInt` string needs the
  predicate to carry the column type. `@default(auth().x)` already accepts it.
- **Unify nullable-column `FieldNeAuth` semantics before C.** A `NULL`
  `Int?`, `Boolean?` or `String?` column against a claim satisfies a create-path
  `!=` while SQL denies. `NullBigInt` is stricter, so the codemod would change
  outcomes.
- **Null-valued `Float`, `DateTime`, `Decimal`, `Json`, `Bytes` and `Uuid`
  procedure arguments in policies** reach the evaluator as `Null` and still pass
  `!=` (in procedure policies only an integer against a string is undecidable).
  An issue is being filed.
- **An absent or unbindable auth claim** (null, float, bytes, list, map) makes a
  column comparison `FALSE` on create, read, update and delete, so
  `@deny(owner != auth().accountId)` stays silent for a caller whose claim is
  null or a float. A `NULL` `Bool` column under `FieldIsTrue` is the same family.
  Pre-existing; a decision. Changing it is one coordinated change in
  `crates/cratestack-sqlx/src/query/support/policy_predicate.rs`,
  `render/policy_predicate.rs` and `query/support/create.rs`.

Found by the end-to-end tests, both pre-existing and not `BigInt` bugs:

- **The migrate emitter copies `@default(auth().x)` into the column as
  `DEFAULT auth().x`**, which is invalid SQL (SQLSTATE 42601), for any scalar.
  It should skip auth-derived defaults.
- **A foreign-key violation (SQLSTATE 23503) is not mapped to a 4xx.** It answers
  `500`, for an `Int` key as much as a `BigInt` one.

TypeScript:

- `packages/cratestack-adapter-rtk` still declares `@reduxjs/toolkit` `^2.0.0`;
  the generated clients now require `^2.2.7`.
- `just verify-typescript-floors` does not exercise RTK, so the new floor is
  pinned only by `tests/bigint_query_keys.rs`.
- Refine: `withRefineId` (`packages/cratestack-refine/src/index.ts:58-65`) casts a
  record's key to `BaseKey`, and a `BigInt` id is a revived `bigint`. It works at
  runtime; returning `String(id)` for a `bigint` is the type-correct option.

Filed, pre-existing, not caused by `BigInt` (open):

- [#1153](https://github.com/cratestack/cratestack/issues/1153): the CBOR codec
  ignores bytes after the first item, where the JSON codec refuses them.
- [#1154](https://github.com/cratestack/cratestack/issues/1154): `generate-typescript
  --swr` uses `<Model>ComputedParams` in `*.hooks.ts` without importing it.
- [#1155](https://github.com/cratestack/cratestack/issues/1155): the generated
  TypeScript client leaves `Decimal[]` fields as strings at runtime.

Build and CI:

- **`cratestack-core`'s `rusqlite` feature** is enabled by `cratestack-rusqlite`
  but nothing on the embedded path needs it; only that crate's unit tests do
  (ADR 0019, Amendment). Keep it for hand-written rusqlite code or remove it.
- `just verify-dart` generates the `bigint_scalar` and `bigint_scalar_rpc`
  fixtures in the default preset only. The Riverpod preset's pinned
  `riverpod_generator` does not solve on every Flutter channel, so the BigInt
  Riverpod legs run in `cargo test -p cratestack-client-dart --test bigint_round_trip`
  (with `CRATESTACK_RELAX_RIVERPOD_PINS=1` where needed), not in that recipe.
- Human-test issues: #1159 (Flutter devices and web), #1160 (TypeScript in browsers), #1161 (downstream upgrades), #1162 (Studio UI), #1163 (VS Code), #1164 (MCP with a real AI client), #1165 (live WireMock)

Ruled out in the ADR, so it isn't proposed again:

- **`BigInt` as a native CBOR integer.** Batch frames pass through
  `serde_json::Value`, the Dart web codec rounds at its JS JSON boundary, and the
  two JS CBOR bridges disagree about large integers. Coming back means a new ADR
  that reworks those three paths first.
- **`BigInt` as an RFC 8949 bignum (tag 2 or 3).** Every `i64` already has a
  native integer form, so a bignum is a second, non-preferred spelling of it, and
  batch frames, the Dart boundary and the bridges have nowhere to put a tag.
- **A 53-bit `Int`** (a 64-bit column constrained to ±(2^53 - 1)). It makes
  correctness depend on validation at every boundary, and no column type has that
  range.
- **A string-encoded `Int`** (keep `i64`, change only its encoding). There would
  be no `BigInt`, every counter and page size becomes a `bigint` in TypeScript,
  and `int4` stays unmapped.

### 🐞 Open bugs

- [#879](https://github.com/cratestack/cratestack/issues/879) — the governance
  check treats a `#` inside a fenced code block as a heading and truncates the
  section.

---

## Database engines — the decision

**We investigated adding SeaORM and Diesel alongside sqlx. Both are ruled out.
sqlx stays. What ships instead is more *dialects*, not more ORMs.**

This is the part of the roadmap most likely to be proposed again by a
well-meaning newcomer, so the reasoning is recorded in full.

### Why not SeaORM

**SeaORM is built on top of sqlx.** Adding it would put a second layer above the
driver CrateStack already uses — it brings no new database, no new driver, and
no capability the framework doesn't already generate.

Worse, SeaORM's actual value is entity codegen, `ActiveModel`, and relation
loading. That is *precisely* what `cratestack-macros` already generates from
`.cstack`. Adopting it means two code-generation systems competing to own the
same typed surface, and the framework's whole thesis is that the schema owns it.

**Verdict: throw away.** No user-visible benefit, direct architectural conflict.

### Why not Diesel

Diesel is a genuinely different stack — its own native drivers, sync-first
(`diesel-async` for async), and the fastest compile times of the three. That
makes it a more interesting candidate than SeaORM, and it still doesn't fit.

Diesel's core value is a type-level query DSL that catches errors at compile
time. But CrateStack doesn't build queries through a DSL — `cratestack-sql`
renders SQL as strings and `Dialect` supplies the placeholder syntax. To use
Diesel we'd bypass the DSL entirely and keep it as a connection pool, paying its
whole dependency and `table!`-macro surface for none of its benefit. Diesel also
wants to own the schema definition, which collides with `.cstack` the same way
SeaORM's entities do.

**Verdict: throw away** — unless a concrete requirement appears that only
Diesel's native drivers satisfy (a target sqlx can't reach). Revisit then, not
before.

### Why sqlx stays

Because the actual demand behind "support more engines" is **MySQL/MariaDB and
server-side SQLite**, and sqlx already drives all of them. The framework's own
error message has promised this since early on:

```
unsupported db backend `{other}`. supported: Postgres, None.
(MySql / sqlite-via-sqlx will land in a future release.)
```

`ServerDb` in `crates/cratestack-macros/src/include/schema_args.rs` is already
an enum wired so that adding a variant is non-breaking at call sites that pass
`db = Postgres`. `provider = "mysql"` already appears in parser tests and
`Mysql` is already a variant in Studio's config. The seams exist.

### What a second dialect actually costs (measured, not estimated)

I measured the coupling rather than guessing at it:

| Signal | Count | Meaning |
| --- | --- | --- |
| `sqlx::` refs in `cratestack-sqlx` | 760 across 82 files | contained, as intended |
| `sqlx::` refs in `cratestack-macros` | 42 across 10 files | **this is the real coupling** — codegen emits `FromRow<'_, PgRow>`, `sqlx::Row`, `sqlx::Error`, `PgPool` |
| `sqlx::` refs in `cratestack-sql` | 2 | **doc comments only** — the dialect-agnostic layer is genuinely agnostic |
| `PgRow` / `PgPool` / `PgTypeInfo` etc. | 97 / 26 / 9 | the concrete types a second backend must abstract |
| `RETURNING` | 83 | 🔴 **MySQL has no `RETURNING`** (MariaDB 10.5+ does) |
| `ON CONFLICT` | 43 | 🔴 MySQL spells this `ON DUPLICATE KEY UPDATE` |
| `MATERIALIZED` | 17 | 🔴 MySQL has no materialized views |
| `Dialect` trait methods | **1** (`write_placeholder`) | the abstraction is currently one method wide |

The good news is the layering holds: `cratestack-sql` is clean, and 86% of sqlx
usage is inside the backend crate where it belongs.

The honest news is that `Dialect` having exactly one method is not evidence the
job is nearly done — it's evidence the job hasn't started. Its own doc comment
says so deliberately: *"Kept deliberately narrow — adding methods here forces
every backend to implement them."* The three 🔴 rows are where the work is.
Losing `RETURNING` in particular means every create/update path needs a
`LAST_INSERT_ID()`-plus-reselect strategy, inside a transaction, with different
semantics under concurrency.

**This is a real project, not a feature flag.** Anyone who scopes it as "add a
variant to the enum" has not read the three red rows.

### 🔵 Needs a decision

- **Is MySQL/MariaDB support actually wanted**, or is Postgres-plus-embedded-
  SQLite the intended shape of the framework forever? Everything above assumes
  the promise in that error message is real. If it isn't, the honest fix is to
  delete the promise, not to keep it as a permanent IOU.
- **If yes: MySQL or MariaDB first?** They diverge exactly where it hurts —
  MariaDB has `RETURNING`, MySQL doesn't. Starting with MariaDB is dramatically
  cheaper and may create the illusion the harder half is done.
- **Server-side SQLite via sqlx** is a much smaller job than MySQL (SQLite has
  `RETURNING` since 3.35 and `ON CONFLICT`) and would let one schema run a
  single-tenant server without Postgres. Worth doing first?

---

## Under consideration

Sourced from `docs/design/`, ADR statuses, and the reserved-ADR list. None are
committed.

### Architecture

- **[ADR 0016 — Store SPI scope](docs/adr/0016-store-spi-scope.md)** is at
  **Proposed**. How far the store SPI should reach is an open architectural
  question with a written analysis and no decision. 🔵
- **ADR 0006–0010 are reserved but unwritten**: COSE envelope modes, migration
  strategy, relation loading, privileged operations, multi-framework support.
  The ADR index already warns the *titles* are stale. Each is a real decision
  someone eventually has to make. 🔵

### Capability gaps

Comparing against what mature schema-first frameworks in other ecosystems ship
(ZenStack v3 was the specific prompt for this review — see below), three gaps
stand out that CrateStack genuinely lacks:

- **Polymorphism / model inheritance.** CrateStack has `mixin` + `@use(...)`,
  which is field reuse, not a type hierarchy. There's no delegate-model or
  single-table-inheritance story. This is the largest modelling gap.
- **OpenAPI emission.** `docs/design/wiremock-stubs.md` notes "no OpenAPI
  emitter" exists. The schema already carries everything one needs, and the
  WireMock emitter proves the traversal works.
- **Client capability slicing.** CrateStack has server-side route suppression;
  narrowing what a *generated client* can express is a different axis.

The React Native and Redux gaps that review also surfaced are no longer
"under consideration" — they are scoped in
[#893](https://github.com/cratestack/cratestack/issues/893).

### Deferred / known

- `application/cbor-seq` is documented as a target transport mode and is not
  implemented.
- Generated routers enforce a single configured codec rather than negotiating
  per request.
- Exact typed non-Rust client generation across arbitrary projection shapes is
  still stabilizing.
- The embedded backend does not enforce `@@allow` / `@@deny`. **This is a design
  decision, not a gap** — clients are untrusted; authorization is the server's
  job. It is listed here only because it gets reported as a bug.

---

## What we looked at elsewhere

**ZenStack v3** (TypeScript) was reviewed as input to this roadmap. It's a full
rewrite that dropped Prisma as a runtime dependency in favour of an engine built
on Kysely, and ships: a Prisma-compatible schema language, built-in access
control and validation, custom `procedure`s implemented in TypeScript,
query-as-a-service, TanStack Query hooks, database introspection (`db pull`),
ORM client slicing, a redesigned plugin system, and polymorphism.

Mapped against CrateStack, most of that list is **already shipped here**:
procedures, access control compiled into SQL, computed fields, auto CRUD APIs,
TanStack Query hooks (plus SWR, Riverpod, RTK, Refine), live-database
introspection and baselining, an extensions/plugin surface, and Zod derivation.

The three things it has that we don't are the capability gaps listed above —
**polymorphism**, **OpenAPI**, and **client slicing**. Polymorphism is the one
worth taking seriously.

One architectural contrast is worth recording rather than copying. ZenStack v3
pairs a high-level ORM with a raw Kysely query-builder escape hatch. CrateStack
took a different route in 0.11.0: `query` blocks are declarative, parameterized,
and **schema-authored**, so the escape hatch stays inside the contract instead of
routing around it. That's a deliberate divergence, not a missing feature.

*(This section exists because the question "what are we missing" deserves a real
answer. It is not a positioning claim, and the README deliberately makes none.)*

---

## The road to 1.0

🔵 **Undecided, and genuinely a maintainer call.**

There is currently no written definition of what 1.0 means for CrateStack — no
milestone, no criteria, no target. Given the release cadence (eleven minor
lines, 0.2 through 0.12, in under four months) and that breaking changes still
land in minors, the useful question isn't "when" but "what has to be true".

Candidate gates, offered as a starting point and **not** as a decision:

- [ ] The four OpExecutor slices land, so an operation is visible whole at L3.
- [ ] Wire-contract stability commitment — what may break in a minor after 1.0?
- [ ] The `db =` promise resolved either way (MySQL ships, or the promise is withdrawn).
- [ ] ADR 0016 decided; ADR 0006–0010 written or formally retired.
- [ ] Non-Rust client generation exact across arbitrary projection shapes.
- [ ] A deprecation policy, since four facades and 15 npm packages version together.

---

## How to influence this

- **Something here matters to you?** Comment on the linked issue, or
  [open one](https://github.com/cratestack/cratestack/issues/new/choose).
  Real use cases move priorities more than anything else.
- **Want to build one of these?** Start with
  [Your first contribution](docs/contributing/first-contribution.md), then say so
  on the issue before you start.
- **Think something here is wrong?** That's a documentation issue and a welcome
  one. This file is maintained by hand and will drift.

Nothing on this page is a commitment. Items marked 🔵 are open questions, not
plans — if you need one resolved to build something, say so and it gets decided.
