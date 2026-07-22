# Migration baselining against an existing live schema — design spike

Status: **proposed** (2026-07-22) — no code shipped yet. This document is the
output of the spike requested in issue [#135][135]; it does not implement the
feature. See "Phasing" (§7) for the follow-up PR sequence.

Scope: `cratestack-migrate`, `cratestack-cli migrate`, `cratestack-sqlx`
migration runner, and (for Phase D) the "adopting an existing database"
walkthrough in `cratestack-docs`.

[135]: https://github.com/cratestack/cratestack/issues/135

## 0. Problem, in one paragraph

`cratestack migrate diff` computes `diff(prev, next)` where `prev` comes from
a committed JSON snapshot (`migrations/<backend>/schema.snapshot.json`) and
defaults to [`Snapshot::empty()`][snapshot-empty] when no snapshot file
exists yet. Pointed at a database that already has tables — created by hand,
by a previous non-cratestack tool, or by a prior internal migration system —
`diff` has no way to know that. It diffs against the empty schema and emits a
full `CREATE TABLE` for everything, because from the tool's point of view
nothing exists yet. There is currently no code path anywhere in this repo
that reads the *actual* state of a live database and treats it as a starting
point. This reportedly reversed an internal ADR (ADR-0003) for one
integrator mid-migration, because there was no way to adopt cratestack
against an already-populated database.

[snapshot-empty]: ../../crates/cratestack-migrate/src/snapshot.rs

## 1. Current architecture (what exists today)

Traced through the code, not from memory — file references throughout.

- **`cratestack-migrate` never touches a database.** Its only inputs are two
  in-memory `cratestack_core::Schema` values: `prev` (read from the snapshot
  JSON, or `Snapshot::empty()`) and `next` (parsed fresh from the `.cstack`
  file). [`diff::diff(prev, next)`](../../crates/cratestack-migrate/src/diff.rs)
  projects both into `BTreeMap<String, TableProjection>` via
  [`convert::project_model`](../../crates/cratestack-migrate/src/convert.rs)
  and walks the two maps to produce `Vec<Op>`. The crate's `Cargo.toml` has
  zero DB dependencies (`cratestack-core`, `serde`, `serde_json`, `thiserror`
  only) — introspection cannot be bolted on without adding one.
- **The snapshot is the only notion of "previous state," and it's a file,
  not a query.** [`snapshot.rs`](../../crates/cratestack-migrate/src/snapshot.rs)
  reads/writes `Snapshot { format_version, schema: Schema }` as pretty JSON.
  `cratestack migrate diff` ([`diff_cmd.rs`](../../crates/cratestack-cli/src/migrate/diff_cmd.rs))
  reads it with `read_or_empty`, diffs, emits DDL, then overwrites it with
  `next_schema` — there's no concept of "this snapshot was established by
  introspecting a live DB" vs. "this snapshot is the result of the last
  generated migration." Both look identical on disk today.
- **`cratestack-cli migrate` has exactly one subcommand: `diff`.** Per its
  own module doc — [`migrate.rs`](../../crates/cratestack-cli/src/migrate.rs):
  *"Slice 5 ships `diff`. `verify` (replay against ephemeral DB) and `drift`
  (introspect live DB) land in subsequent slices."* Neither exists yet. The
  CLI crate has no async runtime and no `sqlx` dependency at all — it is a
  synchronous `anyhow::Result<()>` tool end to end.
  ([`cli_types.rs`](../../crates/cratestack-cli/src/cli_types.rs) `MigrateAction`
  has one variant, `Diff`.)
- **`cratestack-sqlx` tracks *applied* state, separately, with no baseline
  concept.** [`migrations.rs`](../../crates/cratestack-sqlx/src/migrations.rs)
  is the forward-only runner: a `cratestack_migrations` table keyed by
  migration `id`, storing a SHA-256 checksum of `id + description + up`.
  `status()`/`apply_pending()` walk a `&[Migration]` list the caller
  constructed (from `up.sql` files on disk, one imagines, though nothing in
  this repo currently wires migration-directory reading into it — that glue
  is presumably application-side) and compare against DB rows by `id`. There
  is no row type or column that means "everything up to here was true
  before cratestack started managing this database" — baselining needs one.
  This is also not wired into the CLI at all today; the CLI has no `apply`
  or `status` subcommand.
- **Partial introspection exists, but it's UI-only and far too shallow to
  reuse directly.** `cratestack-studio`'s drift indicator —
  [`api/drift.rs`](../../crates/cratestack-studio/src/api/drift.rs) +
  [`data/postgres.rs::inspect_columns`](../../crates/cratestack-studio/src/data/postgres.rs) —
  queries `information_schema.columns` for `column_name, data_type,
  is_nullable` per model and reports missing/extra *column names* for a
  "needs a migration" badge in the UI. It does not look at types precisely,
  defaults, primary keys, indexes, CHECK constraints, enums, or views, and
  it doesn't produce anything `diff()`-shaped — it's a name-set comparison,
  not a schema comparison. It is useful prior art for pool wiring
  (`sqlx_core` + `sqlx_postgres`, no `tokio`-in-CLI precedent problem since
  Studio is already async), but not reusable as the baseline's introspection
  engine.
- **Grep confirms there is no `information_schema`/`pg_catalog` code
  anywhere else in the repo** beyond that Studio drift indicator and the
  `banking_migrations.rs` integration test fixture. Full catalog
  introspection (columns with precise types/defaults/PK, indexes, CHECK
  constraints, enums, views) does not exist in any crate today.

## 2. Why this is genuinely spike-sized, not a single PR

The issue's own "Risks" section called this correctly. Concretely, in order
of how much they change the shape of the solution:

1. **`diff()`'s public contract is `Schema → Schema`, not `IR → IR`.** The
   projection step (`.cstack` model/field/attribute parsing →
   `TableProjection`) is baked inside `diff()`. A baseline can never produce
   a `cratestack_core::Schema` — that type carries source-level constructs
   introspection cannot recover: `mixins`, `procedures`, `views` (as
   authored SQL, not just their catalog shape), `auth`, `transport rpc`
   declarations, and attribute provenance (see point 2). So the diff engine
   needs a second, lower-level entry point that operates on
   `BTreeMap<String, TableProjection>` (or an equivalent introspection IR)
   directly, with the existing `Schema → Schema` path becoming a thin
   wrapper around it. This is a real refactor of `diff.rs`/`convert.rs`, not
   a new file dropped alongside them.
2. **CHECK constraints and column types are lossy to reverse.** `ColumnType`
   in the IR is `Scalar(String)` — the `.cstack` scalar name (`"String"`,
   `"Uuid"`, …), not a SQL type — deliberately, so new scalars don't require
   touching the IR (see
   [`ir/columns.rs`](../../crates/cratestack-migrate/src/ir/columns.rs)
   doc comment). Introspection gets back Postgres types (`text`, `uuid`,
   `numeric(12,2)`, `timestamptz`, …) and has to guess the `.cstack` scalar
   they came from — ambiguous for anything numeric/decimal-precision-shaped,
   and impossible in principle for the validator-derived CHECK constraints
   `convert/checks.rs` emits for `@range`/`@length`/`@iso4217` (a raw
   `CHECK (age >= 0 AND age <= 150)` in the catalog cannot be reverse-mapped
   to "this was originally `@range(0, 150)`" — the SQL is the same either
   way once the attribute is compiled away). §5 proposes treating this class
   of ambiguity as reported drift rather than attempted reconciliation,
   consistent with the issue's explicit "Out of Scope."
3. **The CLI has no live-DB code path to hang this off of.** Adding
   `migrate baseline` means giving `cratestack-cli` its first async runtime
   and DB dependency. Precedented elsewhere in the workspace
   (`cratestack-sqlx`, `cratestack-studio`) but new for this crate — worth
   flagging as a dependency-surface change, not just a new subcommand.
4. **Baseline state has to be legible to *two* different consumers that
   don't currently talk to each other**: `cratestack-migrate`'s
   file-snapshot world (governs what `migrate diff` treats as "already
   there") and `cratestack-sqlx`'s `cratestack_migrations` table (governs
   what the *runtime* applier treats as "already applied"). A baseline that
   only updates the snapshot fixes `diff` but leaves the runner's history
   table blank, so a fresh `apply_pending()` against the same database would
   still try to run the very first real migration from scratch against
   tables that already exist. Both sides need to move together. §5.3 below
   is the fork in the road this doc most wants review on.

None of these are individually unsolvable, but each is a real design
decision with more than one defensible answer, which is exactly the
situation issue #135 asked to be spiked before committing to an approach.

## 3. Prior art (as cited in the issue)

- **Prisma — `prisma migrate resolve --applied <name>`.** Marks a specific
  migration as applied in Prisma's `_prisma_migrations` table *without
  running its SQL*. The developer is expected to first generate a migration
  that matches the live schema (often via `prisma db pull` to introspect,
  then a manual diff-and-reconcile pass) and only then mark it resolved.
  Drift between the "resolved" migration's SQL and the actual live schema
  is entirely the developer's responsibility to have already checked —
  Prisma does not verify it at resolve time.
- **Rails — `rails db:schema:load` + manual baseline.** Rails' answer to
  "adopt an existing DB" is closer to "don't" — `schema:load` is normally
  used to *build* a fresh DB from `schema.rb`, and the documented adoption
  path for an existing production DB is to hand-edit `schema_migrations` to
  insert rows for migrations whose DDL already happened, with no tooling
  support for verifying the live shape matches what the inserted rows imply.
- **What we take from both:** neither tool verifies the live schema against
  the "resolved" state at baseline time — the operator asserts it. Issue
  #135's acceptance criteria go further than either by asking for a **drift
  report** at baseline time (§5.2), which is the one place this design
  should exceed prior art rather than just copy it.

## 4. Naming

`cratestack migrate baseline` — matches the issue's own suggested name and
the `_prisma_migrations`/schema_migrations "baseline" vocabulary in both
prior-art systems, so operators coming from either tool land on a familiar
term. `adopt` and `resolve` were considered; `resolve` in particular is
confusing alongside `resolve --applied`'s per-migration semantics in
Prisma, since this command establishes a *single* starting point rather
than resolving individual pending migrations.

## 5. Proposed design

### 5.1 Split `diff()` into projection + comparison

Extract the existing `project_tables` (already in `diff.rs`, already
building `BTreeMap<String, TableProjection>`) into a public seam, and change
`diff()`'s core loop to take two `BTreeMap<String, TableProjection>` values
plus the enum/view lists it also needs, rather than two `Schema`s directly.
`pub fn diff(prev: &Schema, next: &Schema)` becomes a thin wrapper that
projects both sides and calls the new `diff_projections(...)`. No behavior
change for existing callers or tests — this is a pure refactor and should
land as its own PR (Phase A, §7) provable by "every existing `cratestack-migrate`
test passes unchanged."

### 5.2 Introspection module (new)

A new module — proposed home `cratestack-migrate::introspect::postgres`,
gated behind a `postgres-introspect` feature so the pure-diffing core stays
DB-dependency-free for consumers that don't need it (e.g. anything using
just the SQLite emitter) — that takes a live `sqlx::PgPool` and produces the
*same* `BTreeMap<String, TableProjection>` (+ enums, + views) shape that
`project_tables` produces from a `.cstack` schema, by querying:

- `information_schema.tables` / `pg_class` — table list (respecting
  `current_schema()`, matching Studio's existing pattern).
  `cratestack_migrations` itself must be excluded from the result set.
- `information_schema.columns` + `pg_attribute` — column name, type,
  nullability, default expression, ordinal position.
- `pg_index` + `pg_class` + `pg_attribute` — indexes and their uniqueness,
  matched against `naming.rs`'s existing index-naming convention where
  possible so a baselined-then-later-diffed index doesn't look renamed.
- `pg_constraint` (`contype = 'p'`) — primary key columns.
- `pg_constraint` (`contype = 'c'`) — CHECK constraints, captured as opaque
  SQL text (`pg_get_constraintdef`), *not* reverse-mapped to a validator
  attribute (see §2.2 — this is intentionally lossy and surfaced as drift).
- `pg_enum` + `pg_type` — enum types and their variants.
- `pg_views` — view definitions (`pg_get_viewdef`).

Postgres type → `.cstack` scalar name is necessarily a best-effort,
documented mapping table (e.g. `text`/`varchar` → `String`, `uuid` → `Uuid`,
`timestamptz` → `DateTime`, `boolean` → `Bool`, integer widths → `Int`),
with anything it can't confidently map (arbitrary `numeric(p,s)`, `jsonb`,
domains, extension types) reported as an **unmapped column** — treated as
drift requiring human review rather than silently guessed at. This keeps
faith with the issue's explicit "Out of Scope: automatic schema-drift
reconciliation."

### 5.3 Where baseline state lives — the open fork

Two workable options; this doc recommends (b) but wants sign-off before
Phase C starts, since it's the one architectural choice that's expensive to
reverse later.

**(a) Snapshot-only.** `migrate baseline` writes
`migrations/<backend>/schema.snapshot.json` from the introspected shape (via
a synthetic `Schema` built only as far as the snapshot's serialization needs
— note this pushes back into the same lossiness problem as §2.1 unless the
snapshot format is changed to store the IR directly instead of a `Schema`).
`migrate diff` needs no changes at all — it already reads whatever snapshot
is on disk as `prev`. Simple, but leaves `cratestack-sqlx`'s
`cratestack_migrations` table untouched, so the *runtime* applier still has
no record that pre-baseline state exists. Fine for teams that only ever run
`migrate diff` + hand-apply, broken for teams relying on `apply_pending()`
against the same database later — it would attempt to run the first
generated migration from scratch against tables baseline already accounted
for.

**(b) Snapshot + synthetic runner row (recommended).** Same snapshot write
as (a), *plus* `migrate baseline` (when given a `--database-url`, which
becomes required rather than optional for exactly this reason) inserts one
synthetic row into `cratestack_migrations`:
`id = "<timestamp>_baseline"`, `description = "baseline: adopted N existing
tables"`, `checksum` = a hash over the introspected shape itself (so a
second baseline run against a since-drifted DB is detectable, mirroring the
existing checksum-mismatch story `apply_pending()` already enforces). This
keeps `cratestack-migrate` (authoring) and `cratestack-sqlx` (runtime) in
sync at the moment of adoption, at the cost of `cratestack-migrate` (or the
CLI glue around it) needing to depend on `cratestack-sqlx`'s `Migration`
type / migrations-table schema. Recommended because it's the only option
that satisfies "a subsequent `apply_pending()` run doesn't try to recreate
what baseline already accounted for" — which isn't in the issue's explicit
acceptance criteria but is implied by treating baseline as a real starting
point rather than a `migrate diff`-only concept.

### 5.4 Drift report at baseline time

`diff_projections(introspected_ir, authored_ir)` — the same function from
§5.1, called with the introspected shape as `prev` and the current
`.cstack` schema as `next` — *is* the drift report. No new comparison logic
needed; this reuses the diff engine as-is. `migrate baseline`:

- Runs it, and if the op list is non-empty, prints a human-readable summary
  (grouped by table, using each `Op`'s existing `Destructiveness` — drift
  that would be `Lossy`/`Blocking` if applied is flagged more loudly than
  `Safe` drift like a missing index) and **does not fail** by default,
  per the issue's explicit "report drift, don't hard-fail." An
  `--allow-drift`-style flag is *not* proposed — reporting-not-failing
  should be the unconditional default for this command, since the entire
  point is adopting a database that doesn't match perfectly.
- A `--strict` flag (opt-in, not default) exits non-zero on any drift, for
  teams that want baselining to double as a "prove the schema already
  matches" CI gate rather than an adoption tool.

### 5.5 CLI surface

```
cratestack migrate baseline \
  --schema <path/to/schema.cstack> \
  --database-url <postgres-url> \
  [--out-dir migrations]         # matches `diff`'s default
  [--backend postgres]           # baseline is Postgres-only for v1 — see §6
  [--strict]                     # fail (non-zero exit) if drift is found
```

Refuses to run (non-zero exit, no writes) if a snapshot already exists at
the target path — baselining an already-managed backend is almost certainly
a mistake, and the failure mode of silently overwriting a real migration
history is worse than requiring `rm migrations/postgres/schema.snapshot.json`
first as an explicit, deliberate act.

## 6. Open questions (need a decision before Phase A starts)

1. **§5.3 fork** — snapshot-only vs. snapshot + synthetic runner row.
   Recommend (b); needs sign-off since `cratestack-migrate` gaining a
   dependency on `cratestack-sqlx` types is a layering change worth being
   deliberate about (today the dependency graph flows the other direction —
   `cratestack-sqlx` doesn't currently depend on `cratestack-migrate`
   either, so this would be a new edge, not a reversal, but still worth
   naming explicitly).
2. **Postgres-only for v1?** The issue and the integrator's report are both
   Postgres-specific (ADR-0003 was about a Postgres-backed service). SQLite
   embedded targets have no live "existing database" story the same way (no
   long-lived production DB, no `information_schema` in most cases in the
   same sense) — recommend scoping the introspection engine to Postgres
   only and leaving `--backend sqlite`/`both` out of `migrate baseline`
   until a concrete SQLite adoption story shows up.
3. **Type-mapping table completeness for v1.** Recommend shipping with the
   common scalar set (`String`/`Int`/`BigInt`/`Bool`/`Uuid`/`DateTime`/
   `Float`) and explicitly reporting anything else (`numeric`, `jsonb`,
   arrays, domains, extension types) as unmapped drift rather than trying
   for full coverage in the first cut — matches "report drift, don't
   silently reconcile."

## 7. Phasing — proposed follow-up PRs

Each phase is independently reviewable and independently useful; none is
required to land before the next is *written*, but each is required to be
*merged* before the next starts, since B depends on A's seam and C depends
on both.

| Phase | Content | New tests |
|---|---|---|
| A | Split `diff()` into `Schema→IR` projection + `IR→IR` comparison, `TableProjection`-equivalent made a public seam. Pure refactor, no behavior change. | Existing suite passes unchanged; no new DB code. |
| B | `cratestack-migrate::introspect::postgres` module (behind feature flag) producing IR from a live `PgPool`. Not wired into the CLI yet. | `just test-pg`: introspect a hand-created table, assert the resulting `TableProjection` matches what `project_model` would produce from the equivalent `.cstack` schema. |
| C | `cratestack migrate baseline` CLI command: introspection + drift report (§5.4) + snapshot write + (pending §6.1 decision) runner-row seed. `cratestack-cli` gains its first async/DB dependency. | `just test-pg`, mapped directly to the issue's acceptance criteria — see §8. |
| D | "Adopting an existing database" walkthrough in the separate `cratestack-docs` repository (docs live there, not in this repo). | N/A (docs PR). |

This document itself is the Phase-0 deliverable requested by the issue.

## 8. Test plan (Phase C, mapped to issue acceptance criteria)

All via `just test-pg` (real Postgres, per `compose.yml`):

1. Create tables out-of-band (raw `CREATE TABLE` via `sqlx`, simulating an
   existing DB) matching a `.cstack` schema exactly → `migrate baseline` →
   assert the drift report is empty → assert a subsequent `migrate diff`
   reports "no changes."
2. Create tables out-of-band that *differ* from the `.cstack` schema (extra
   column, missing index, mismatched nullability) → `migrate baseline` →
   assert the drift report surfaces exactly those differences, grouped and
   attributed to the right table/column → assert the command still exits 0
   and still writes the snapshot (report, don't hard-fail).
3. After a clean baseline (case 1), add a field to the `.cstack` schema →
   `migrate diff` → assert the emitted SQL is a correct incremental
   `ALTER TABLE ... ADD COLUMN`, not a `CREATE TABLE` — this is the
   regression test for the bug as originally reported.
4. (If §6.1 resolves to option (b)) After baseline, run
   `cratestack_sqlx::migrations::apply_pending()` against the same database
   with a migrations list that includes only *post-baseline* migrations →
   assert none of the pre-baseline DDL is attempted and the synthetic
   baseline row is visible in `status()`.

## 9. Risk assessment

- **Medium-high.** Live-schema introspection accuracy is the core risk —
  wrong type mapping or missed constraint produces either false drift
  (annoying) or, worse, a baseline that silently under-reports real drift
  (dangerous, since a later `migrate diff` would then treat something
  genuinely different as identical). §5.2's "unmapped → reported drift, no
  guessing" rule is the main mitigation.
- Adding an async DB dependency to `cratestack-cli` is a real but low-risk
  dependency-surface change, precedented by `cratestack-sqlx`/
  `cratestack-studio` already existing in the workspace.
- No risk to existing behavior in Phases A/B (additive, feature-gated, no
  existing call site changes). Phase C is the first phase with user-facing
  surface area (a new command) and should get its own focused review pass
  independent of this design doc.
