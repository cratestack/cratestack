# Composite primary keys (`@@id([...])`) — design + phased plan

Status: **in progress** — schema declaration + migration DDL **shipped**; query
builders, routing, and generated clients **deferred** (gated with a clear
compile error, not a silent panic).
Scope: any model declaring `@@id([field1, field2, ...])` instead of a
single-field `@id`.
Tracking: [#136](https://github.com/cratestack/cratestack/issues/136).

## Shipped vs. pending

| Item | Status | Where |
|------|--------|-------|
| `@@id([field1, field2, ...])` parsing + semantic validation | **shipped** | `cratestack-parser` |
| Mutual exclusivity with field-level `@id`, duplicate/unknown-field/relation-field/readonly/version rejection | **shipped** | `cratestack-parser` |
| Migration DDL: real composite `PRIMARY KEY (a, b)` constraint | **shipped** | `cratestack-migrate` |
| Codegen gate: clear `compile_error!` instead of a panic when `@@id` reaches `include_server_schema!`/`include_embedded_schema!`/`include_client_schema!` | **shipped** | `cratestack-macros` |
| Query builders (`find_unique`/`update`/`delete`/`upsert`) accept a composite key | **pending** | `cratestack-sql`, `cratestack-sqlx`, `cratestack-rusqlite` |
| Axum route + RPC envelope shape for composite-key CRUD | **pending** | `cratestack-macros::axum`, `cratestack-macros::transport::rpc`, `cratestack-axum::rpc::inputs` |
| Generated clients (Rust/Dart/TypeScript) `get`/`update`/`delete` taking multiple key fields | **pending** | `cratestack-macros::client::{rest,rpc}`, `cratestack-client-dart`, `cratestack-client-typescript` |
| Relation matching (`@relation(fields:[...], references:[...])`) against a composite target PK | **pending** | `cratestack-macros::relation::types` (`relation_link`) |
| Policy relation/quantifier traversal through a composite-PK junction table | **pending** | `cratestack-policy`, `cratestack-sqlx::render::policy*`, `cratestack-rusqlite::render::relation` |
| Views (`@id` cardinality rule) composite equivalent | **pending, likely unnecessary** | `cratestack-parser::validate::views` — views already support opting out via `@@no_unique`; revisit only if a concrete need appears |

The four shipped items fully satisfy the "declare it, get correct DDL,
never lose data" slice of the original issue, without touching the parts of
the framework that assume a single scalar PK type end-to-end. That
assumption turned out to be load-bearing far beyond what the issue
anticipated — see §1.

## 1. Why this is phased

The single-PK assumption is not a handful of `if` checks; it is a generic
type parameter, `ModelDescriptor<M, PK>` (and its cousins `ReadSource<M, PK>`,
`ViewDescriptor<V, PK>`, `ModelPrimaryKey<PK>`, `RpcPkInput<Pk>`,
`RpcUpdateInput<Pk, Patch>`), threaded through **generated code** in every
backend and every client. Widening `PK` from a scalar to "one or more named
columns" is a shape change, not a value change — every one of these
generics needs to become either a generated tuple/struct type or be
re-expressed as a named-field key. That cannot be done safely without also
updating:

- every hand-written trait in `cratestack-sql`/`cratestack-sqlx`/`cratestack-rusqlite`
  that is generic over `PK` (≈15 files),
- every codegen call site in `cratestack-macros` that does
  `model.fields.iter().find(is_primary_key)` for exactly one field
  (≈12 files: descriptor, accessor, inputs, axum prep/builders/handlers,
  RPC transport, relation FK matching, procedure `@authorize` type-checking),
- axum route shape (`Path<PK>` doesn't extend to a tuple without either a
  custom extractor or a route redesign, e.g. `/model/:f1/:f2` vs. a single
  opaque segment),
- three independent client generators, two of which are Jinja2-templated
  (`cratestack-client-dart`, `cratestack-client-typescript`) and one of which
  is `quote!`-based (`cratestack-macros::client::{rest,rpc}`), each with its
  own `primary_key_type`/`is_primary_key` derivation and its own golden-file
  snapshot suite,
- the policy engine's relation-traversal SQL emitters, which today
  interpolate exactly one `parent_column`/`related_column` pair per hop
  (`ReadPredicate::Relation`, `RelationFilter`) — a composite FK relation
  needs an `AND`-joined list of column-pairs instead of one equality.

None of this is hidden or deferred behind a flag — it simply hasn't been
built yet. A model using `@@id([...])` fails loudly and immediately at
`include_*_schema!` expansion time with a message pointing at this doc and
the tracking issue, rather than silently generating incorrect SQL or
panicking deep inside a `.expect()` with no context.

## 2. What's already there, concretely

### 2.1 Parser (`crates/cratestack-parser`)

- `crates/cratestack-parser/src/validate/model_attributes.rs` — new
  `validate_composite_id_attribute`, following the same per-model `@@`-attribute
  validation pattern as `@@paged`/`@@retain`. Enforces:
  - well-formed `@@id([a, b, ...])` syntax (via
    `cratestack_core::parse_composite_id_attribute`),
  - at least two fields (a single-field composite key should use `@id`),
  - no duplicate field names,
  - mutual exclusivity with any field-level `@id`,
  - every listed field exists, is scalar (not a `@relation` field), and
    does not carry `@readonly`/`@server_only`/`@version`,
  - at most one `@@id(...)` attribute per model.
- `crates/cratestack-parser/src/validate/models.rs` — the "model must have a
  primary key" check now accepts either a field-level `@id` or a validated
  `@@id(...)` attribute.
- `crates/cratestack-core/src/schema/composite_key.rs` — the parsing
  primitive (`parse_composite_id_attribute`), mirroring
  `cratestack_core::events::parse_emit_attribute`'s shape so the syntax
  lives in one place any consumer can share.

### 2.2 Migration DDL (`crates/cratestack-migrate`)

- `crates/cratestack-migrate/src/convert.rs` — `project_model` now marks
  every column listed in a model's `@@id([...])` attribute as
  `Column { primary_key: true, .. }`.
- **No emitter changes were needed.** `emit_create_table` in both
  `crates/cratestack-migrate/src/emit/postgres/tables.rs` and
  `crates/cratestack-migrate/src/emit/sqlite/tables.rs` already collects
  *every* `primary_key`-flagged column into one trailing
  `PRIMARY KEY (col1, col2, ...)` constraint clause — it was written
  column-list-first even though only single-column lists existed before.

This means schema authors can adopt `@@id([...])` today for the DDL/migration
half of their workflow — real composite `PRIMARY KEY` constraints, no
synthetic surrogate column — while the ORM/client/policy layers catch up.

### 2.3 The codegen gate (`crates/cratestack-macros`)

- `crates/cratestack-macros/src/include/parse.rs` —
  `reject_composite_primary_keys`, called once from the single
  `parse_schema_literal` function all three top-level macros
  (`include_server_schema!`, `include_embedded_schema!`,
  `include_client_schema!`) funnel through. One check, one message, instead
  of patching ~12 individual `.find(is_primary_key).expect(...)` call sites
  each with their own ad-hoc panic text.

## 3. Remaining phases (proposed follow-up PRs)

Each phase below is intended to be its own PR, each removing one row from
the "pending" table above and narrowing the `reject_composite_primary_keys`
gate accordingly (e.g. once query builders support composite keys but axum
routing doesn't yet, the gate moves from "reject always" to "reject unless
consumed only via the typed Rust accessor, not axum/RPC/clients" — exact
gating to be decided when that phase starts).

### Phase A — `cratestack-sql` / `cratestack-sqlx` / `cratestack-rusqlite`: composite key type + query builders

- Introduce a `CompositeKey`-shaped alternative to the scalar `PK` generic —
  most likely a per-model generated tuple type (`(FieldTy1, FieldTy2)`) or a
  small generated struct with named fields, produced by
  `cratestack-macros::model::descriptor`.
- Update `ModelDescriptor<M, PK>` (`crates/cratestack-sql/src/descriptor/mod.rs`),
  `ReadSource`/`WriteSource` (`descriptor/read_source.rs`), and
  `ModelPrimaryKey<PK>`/`UpsertModelInput` (`values/input_traits.rs`) so `PK`
  can be instantiated with either a scalar or a composite key type.
  `primary_key: &'static str` (single column name) becomes
  `primary_key_columns: &'static [&'static str]`.
- Update every query builder that assumes one PK column: the batch/read/write
  modules under `cratestack-sqlx/src/query/{batch,read,write}/*.rs` and their
  `cratestack-rusqlite/src/{batch,render}/*.rs` equivalents — `WHERE`-clause
  and `ON CONFLICT`/upsert-conflict-target construction both need an
  `AND`-joined multi-column predicate instead of `col = $1`.
- Test with real Postgres via `just test-pg` — extend the `AccountMembership`-shaped
  fixture pattern already used in `crates/cratestack-pg/tests/`.

### Phase B — axum routing + RPC transport

- REST: composite-key routes need either a multi-segment path
  (`/account-memberships/:account_id/:subject`) with a custom axum extractor,
  or a different addressing scheme entirely (e.g. always POST a body for
  composite-key models). Decide this before touching
  `cratestack-macros/src/axum/model/{handlers_crud,handlers_update,prep,builders}.rs`.
- RPC: `RpcPkInput<Pk>`/`RpcUpdateInput<Pk, Patch>`
  (`crates/cratestack-axum/src/rpc/inputs.rs`) need `Pk` to carry multiple
  named fields — likely a per-model generated struct rather than widening
  the envelope itself. Update `cratestack-macros/src/transport/rpc.rs`
  dispatch-arm generation to match.
- `cratestack-macros/src/procedure/authorizer.rs`'s `@authorize` PK-type
  check becomes a PK-*shape* check (field set equality, not one type
  equality).

### Phase C — generated clients (Rust, Dart, TypeScript)

- Rust: `cratestack-macros/src/client/{rest,rpc}/model.rs` — `get`/`update`/`delete`
  take the generated composite-key type/struct instead of `&#primary_key_type`.
- Dart: `cratestack-client-dart/src/{naming,builders_model,views}.rs` +
  `templates/{rest-apis,rpc-apis}.dart.j2` — `get(int id, ...)` becomes
  `get(int accountId, String subject, ...)` (or a named-record parameter,
  TBD during implementation — prefer whichever reads more idiomatically in
  each language rather than forcing one shape across all three).
- TypeScript: `cratestack-client-typescript/src/{types,views}.rs` +
  `templates/src/{rest-client,rpc-client}.ts.j2` — same shape change.
- New composite-key fixtures + regenerated golden snapshots in
  `cratestack-client-dart/tests/{fixtures,snapshots}` and
  `cratestack-client-typescript/tests/{fixtures,snapshots}`, following the
  existing hand-rolled `run_snapshot`/`CRATESTACK_UPDATE_SNAPSHOTS=1` harness
  (no `insta` in this repo).

### Phase D — relation matching + policy traversal

- `cratestack-macros/src/relation/types.rs::relation_link` currently hard-rejects
  any `@relation(fields:[...], references:[...])` with more than one field —
  this is explicitly **out of scope** per the issue ("composite *foreign*
  keys... treat as a likely follow-up"), but a composite-PK junction table's
  own two FK relations (to each side of the many-to-many) are ordinary
  single-column relations today and already work; only a *relation whose
  target is itself a composite-PK model* needs this widened, which nothing
  in this issue requires.
- Policy: `ReadPredicate::Relation` (`cratestack-policy/src/read_types.rs`) and
  its three SQL emitters (`cratestack-sqlx/src/render/policy.rs`,
  `cratestack-sqlx/src/query/support/policy_relation.rs`,
  `cratestack-rusqlite/src/render/relation.rs`) each interpolate exactly one
  `parent_column`/`related_column` pair. Traversing *into* a composite-PK
  junction table (e.g. `.some.user.email` through
  `AccountMembership.@@id([accountId, subject])`) doesn't actually need this
  — the join is still on the FK column(s) of the *relation*, not the
  junction table's own PK. This phase is only required if a future relation
  needs to join **on** a model's composite PK (e.g. a relation whose
  `references:[...]` target a composite-PK model) — track separately if that
  need materializes; do not build ahead of a concrete case.
- Add a `policy_db_composite_key.rs` integration test mirroring
  `crates/cratestack-pg/tests/policy_db_recursive.rs`'s structure once
  Phase A/B land, using an `AccountMembership`-shaped fixture with
  `@@id([accountId, subject])` instead of the current synthetic-`id`
  `Membership` fixture.

## 4. Non-goals (per the original issue)

- Composite *foreign* keys spanning multiple columns pointing at a composite
  PK (explicitly out of scope in #136).
- Any gradual/parallel path that keeps a synthetic-`@id` escape hatch once
  `@@id([...])` is available for a given layer — each phase above is a hard
  cutover for that layer, not an opt-in flag.
