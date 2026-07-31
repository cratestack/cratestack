# Changelog

## 0.4.18 (2026-07-31)

### Studio: Postgres row-keying fix, persistent audit log, EXPLAIN

`Row` is documented as keyed by `.cstack` field name, and the UI, cursor
pagination, relation-follow, and audit log all rely on that contract — but
the Postgres data source keyed rows by raw snake_case column name instead.
camelCase and snake_case coincide for single-word fields, which is why this
went unnoticed; on a realistic schema, every multi-word field silently broke
table rendering, pagination's "Next" button, relation follow, and the audit
log's recorded PK. Fixed by aliasing each projected column to its field name.

Also new: an opt-in persistent audit log (`[workspace] audit_file`, an
append-only JSONL sidecar replayed on boot, replacing in-memory-only
history), and query plans (`GET .../sql?explain=true` plus an "Explain"
toggle in the Studio UI). (#240)

### Studio: edit form no longer corrupts NULL columns on save

Opening a row with a NULL nullable column, clicking Edit, and clicking Save
without changing anything wrote the literal string `"—"` (the read-only
table's display placeholder for NULL) into that column instead of leaving it
NULL. The edit-form snapshot was reusing the display-formatting helper to
seed the editable form; it now maps NULL to the same "no value" sentinel
every editor widget already uses, matching what the save path already
expects. (#242)

## 0.4.17 (2026-07-30)

### Parser and migrate hardening around storage-type edge cases

A cluster of related fixes tightening what the parser accepts and what
`cratestack-migrate` emits, found while generating a round-trip test for
every builtin scalar/enum across Postgres, SQLite, and the LSP (#232, #237):

* Postgres now stores enums as `TEXT` + `CHECK` (not a native `CREATE TYPE
  ... AS ENUM`), and bareword enum defaults are quoted correctly in the
  emitted DDL (#233).
* `type` blocks can no longer be used as a model field's storage type —
  they're a payload shape for procedures, not a column type (#235).
* List-arity scalar/enum model fields are rejected on datasource-backed
  schemas, since there's no portable column type for "array of enum" across
  both backends (#229, via #236).
* Reconciled `#233`'s enum-list emitter test with `#229`/`#236`'s new
  list-arity parser rejection — the two landed close together and briefly
  disagreed on enum-list fields (#238).
* `Json` now derives `Default`, fixing a compile failure under
  `include_embedded_schema!` for models with a default-valued `Json` field
  (#234).

### Other fixes

* Rate-limit store errors are logged instead of failing the request
  silently (#215).
* A CI-only quality pipeline (informal replacement for a paid SonarQube
  instance) landed across several follow-up PRs — pinned-action scanners,
  PR review-comment output instead of Check annotations, and a documented
  gap-until-landed note for interim coverage (#216, #218, #220, #222, #225).

### Dart: native gRPC client generator

`generate-dart` gains a native gRPC client generator for schemas declaring
`transport grpc`, plus channel-shutdown and per-call option exposure on the
generated client, and gRPC-specific example/test templates (a pre-existing
RPC-transport example/test bug was caught and fixed during review) (#210,
via #211, #213, #214).

## 0.4.16 (2026-07-26)

No code changes. A clean recut of the release pipeline after v0.4.14 (which
shipped GitHub-Release-only by deliberate choice) and v0.4.15 (crates.io +
GitHub Release succeeded, but both npm publishes failed with `EOTP` — the
configured `NPM_TOKEN` wasn't an Automation-type token). v0.4.16 is the
first release to publish successfully to crates.io, npm (`@cratestack/cli`
and `@cratestack/api`), and GitHub Release binaries in one shot, with zero
manual publish steps.

## 0.4.15 (2026-07-26)

`cut-release-tag.yml`'s tag push now uses a dedicated `RELEASE_PAT` instead
of the default `GITHUB_TOKEN` (#197). GitHub's anti-recursion protection
silently no-ops any downstream workflow trigger from a push made with the
default token — the tag itself lands fine, but `release-cli.yml` never
fires off it. A PAT-authored push is treated as a normal external push and
correctly cascades into the rest of the pipeline.

## 0.4.14 (2026-07-26)

### Protobuf + gRPC support

`.cstack` schemas can now declare `transport grpc`, generating `.proto`
message/enum definitions (with a field-number lockfile so wire numbers
don't silently renumber across schema edits) and gRPC service surfaces.
Design doc (#166) and implementation (across #168–#172) landed same-day
(#167, #176). CRUD-only for this release — procedure/streaming support and
a Rust gRPC client were carved out as follow-up tickets.

### Schema-fingerprint drift header

Every response now carries an `x-cratestack-schema-sha` header — a
warn-only fingerprint of the server's schema, so a client running against
a stale generated SDK can detect drift without a hard version pin. Shipped
for the Rust server first (#179), then Dart and TypeScript REST/RPC clients
(#180).

### RPC client DX: composable link chain

The generated TypeScript RPC client gains a composable `RpcLink` chain
(request/response middleware — logging, batching, auth injection, etc.),
published alongside a new `@cratestack/api` npm package carrying the
batching link and other cross-cutting concerns out of the generated code
itself (#182, #186).

### CI-driven release pipeline

The first version of the fully automated release flow: a `prepare-release`
workflow bumps versions and opens a PR, merging it auto-tags via
`cut-release-tag.yml`, and the tag push triggers `release-cli.yml` to
publish crates.io + npm + GitHub Release binaries with no manual steps
(#188). Landed rough — this version alone needed eight follow-up fixes to
get a real dry run and then a real dispatch through the pipeline end to
end: missing GTK/WebKit deps in CI (#189), the release-check test stage
needing a bundled Studio UI first (#190), `cargo publish --dry-run`
needing `--allow-dirty` (#191) and `--no-verify` (#192) in dry mode, dry
mode needing to skip non-leaf crates entirely since a never-published
version can't resolve as a dependency (#193), and two npm `pnpm install`
call sites needing to skip the `cratestack-cli` binary download since
neither actually needs it (#194, #196). (The pipeline's tag-push
anti-recursion bug that blocked this version's own crates.io/npm publish
is the separate v0.4.15 fix above.)

### Other fixes

* `Cuid` scalar validation relaxed to accept `cuid2` ids, not just the
  original `cuid` format (#150, via #158).
* `cratestack-redis` gains a `tls-rustls` feature for `rediss://`
  connections (#151, via #159), and later in this same version switches
  to caching and reusing a single connection instead of opening one per
  call (#175, decision recorded in #177).
* Design doc proposing an `Extensions` concept, reframing the rate-limiting
  half of #139's declarative-surface decision (#160).
* Clippy `too_many_arguments`/`type_complexity` cleanup in `cratestack-sql`
  and `cratestack-sqlx` (#184, #185).

## 0.4.13 (2026-07-22)

A dense release — nine PRs, several the direct result of a full backlog
pass over long-open tickets:

* **`--check` drift-detection mode** for `generate-typescript` /
  `generate-dart`: exits non-zero if generated output would differ from
  what's on disk, for CI gates (#141).
* **Prebuilt `cratestack-cli` binaries** — GitHub Releases, `cargo-binstall`
  support, and an npm-installable wrapper, so installing the CLI no longer
  requires a Rust toolchain (#142).
* **`--full-selection` flag** for `generate-typescript`, emitting a fully-
  required model type alongside the normal partial-selection type (#140).
* **`cratestack diff`** — a new CLI subcommand that diffs two `.cstack`
  schemas and classifies each change by its effect on the generated wire
  contract (breaking / additive / internal-only), exiting non-zero on any
  breaking change so it can gate CI on schema PRs (#144).
* **Migrate baselining design spike** — a doc-only PR spiking Postgres
  live-schema introspection for baselining an existing database against a
  `.cstack` schema, not yet implemented (#135, via #143).
* **Composite primary keys** via `@@id([...])` — parser and
  `cratestack-migrate` DDL support landed; query builders, clients, and
  policy integration are follow-up work (#145).
* **Idempotency/rate-limiting declarative-surface decision** — a design
  doc settling that rate-limiting stays an imperative, hand-wired concern
  permanently, while idempotency is deferred pending an `OpExecutor` gate
  (#139, via #146).
* **`dbgenerated()` fix** — emits valid SQL instead of a broken default
  expression, and warns when the expression can't be verified against the
  target dialect (#148).
* **Type-block field-reference fix** — qualifies a `type` block's
  references to model types correctly instead of emitting an ambiguous
  reference (#137, via #147).

## 0.4.12 (2026-07-22)

The generated TypeScript RPC client runtime now satisfies its own
`exactOptionalPropertyTypes` compiler setting — a previous release enabled
the stricter TS option in the generated code but the runtime itself wasn't
compliant, so consumers with the same setting on saw type errors (#129).

## 0.4.11 (2026-07-22)

* Fixed `Page<T>`/`PageInfo`'s generated TypeScript shape not matching
  what the wire actually sends (#124).
* Capped the `list` route's page-size limit consistently across REST and
  RPC transports, and made the RPC codec pluggable rather than hardcoded
  (#126, closing #123 and #125).

## 0.4.10 (2026-07-22)

A round of audit-driven correctness fixes: a self-deadlock in the audit
path, a wrong soft-delete snapshot, a server-only field leaking into the
generated TypeScript client, and incorrect gating on TypeScript's generated
`create` calls (#120) — plus a fix for cross-binary test table-name
collisions inside `cratestack-pg`'s own test suite (#121).

## 0.4.9 (2026-06-17)

* Dart's CBOR decoder now normalizes decoded maps to `Map<String,
  Object?>` instead of a more loosely-typed map shape (#115).
* Fixed the `sqlite_offline_first` example failing to compile standalone,
  and guarded the embedded examples in CI (#106).

## 0.4.8 (2026-06-15)

Studio UI chrome revamp: reworked visual chrome and a multi-`.cstack`
target switcher, so one running Studio instance can browse several
schemas' targets from the same UI (#105). The repo also adopted an
AI-governance kit for issue/PR templates and contribution process around
this time (#104).

## 0.4.7 (2026-06-08)

For schemas using `transport rpc`, the op id is now the canonical request
identity — the value request signing and tracing key off, rather than an
incidental routing detail (#102).

## 0.4.6 (2026-06-07)

Fixed `BatchableCall` mis-encoding `None` optionals as a CBOR empty array
instead of a CBOR null in the Rust client (#100).

## 0.4.4 (2026-05-20)

* Published a documentation-only `cratestack` landing crate to crates.io
  — after the umbrella-facade split below removed the real `cratestack`
  crate, this keeps the name from going orphaned/squattable and points
  visitors at `cratestack-pg` / `cratestack-sqlite` (#97, doctests
  disabled on it in a same-day follow-up, #98).
* `CoolError` now preserves the full typed `DatabaseError` chain instead
  of flattening it, so callers can match on the underlying driver error
  (#99).

## 0.4.3 (2026-05-19)

Follow-up to the facade split below: fixed generator-fixture test paths
that still pointed at the removed `cratestack` umbrella instead of
`cratestack-pg` (#96).

## 0.4.2 (2026-05-19)

### Breaking: the `cratestack` umbrella facade was split

The single `cratestack` umbrella crate is gone. It has been carved into
two strictly disjoint sub-facades that consumers pick between via
Cargo's `package =` rename:

```toml
# Backend service (Postgres + Axum + generated Rust client runtime)
cratestack = { package = "cratestack-pg", version = "0.4" }

# Embedded / mobile / desktop / wasm (rusqlite + shared surface)
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

Schema macros (`include_server_schema!`, `include_embedded_schema!`,
`include_client_schema!`) continue to emit `::cratestack::*` paths
unchanged. Strict disjointness is enforced by what the consumer picks,
not by the macro.

**Why this matters in practice:**

* `cratestack-pg` does not pull in `cratestack-rusqlite`, so
  `libsqlite3-sys` is no longer in the dep graph. Backend services can
  now depend on the official `sqlx` umbrella crate (which optionally
  declares `sqlx-sqlite`) without tripping Cargo's `links = "sqlite3"`
  collision rule. Downstream `sqlx-shim` workarounds can be deleted.
* `cratestack-sqlite` keeps compiling on `wasm32-unknown-unknown`; it
  also exposes `cratestack-client-rust` on native targets so hybrid
  consumers (e.g. a Tauri or NAPI shell that ships an embedded DB
  *and* calls a remote backend) can still use `include_client_schema!`
  alongside `include_embedded_schema!`.

### Breaking: `Projection` trait moved + renamed

The `Projection` trait — implemented by every model's macro-emitted
`Selection` type to decode projected query responses — has moved from
`cratestack-client-rust` into `cratestack-core` and been renamed
**`ProjectionDecoder`**. The previous name collided with the SQL value
type `cratestack_sql::Projection<T>` (the actual `.select()` result
wrapper), which was the more central, user-facing meaning of the name.

* Old: `cratestack::client_rust::Projection`
* New: `cratestack::ProjectionDecoder`

`cratestack-client-rust` keeps re-exporting the trait under both
`ProjectionDecoder` and the deprecated `Projection` alias for one
release. Macro-emitted code now references the new name, so most
codebases will see no source-level impact.

### New: SQL views (ADR-0003)

A new `view` block in `.cstack` declares a read-only, SQL-defined
projection over one or more existing `model` blocks. Views generate
a typed Rust struct, a read-only delegate, and `CREATE VIEW` DDL
during migration generation, with the same `@@allow` policy
enforcement models get.

```cstack
view ActiveCustomer from Customer, Order {
  id          Int       @id  @from(Customer.id)
  email       String         @from(Customer.email)
  orderCount  Int

  @@server_sql("""
    SELECT c.id, c.email, COUNT(o.id)::int AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)
  @@embedded_sql("""
    SELECT c.id, c.email, COUNT(o.id) AS order_count
    FROM   customers c
    LEFT JOIN orders o ON o.customer_id = c.id
    GROUP  BY c.id, c.email
  """)

  @@allow("read", auth() != null)
}
```

```rust
let cool = cratestack_schema::Cratestack::builder(pool).build();
let rows = cool.views().active_customer().find_many().run(&ctx).await?;
```

#### Capabilities

* **Both backends.** `@@server_sql` runs against Postgres; `@@embedded_sql`
  runs against SQLite. The `@@sql` shorthand applies to both with a
  cargo warning that portability is the developer's problem.
* **Materialized views (server only).** `@@materialized` emits
  `CREATE MATERIALIZED VIEW` + `CREATE UNIQUE INDEX <name>_pkey ON
  <name> (<id>)` and produces a `refresh()` method on the delegate
  that runs `REFRESH MATERIALIZED VIEW CONCURRENTLY`. Embedded
  builds with a `@@materialized` view hard-error at macro expansion
  time — SQLite has no materialized views.
* **Type-level read-only.** `ViewDescriptor` does not implement
  `WriteSource`, so the bound on `CreateRecord` / `UpdateRecord` /
  `DeleteRecord` / `UpsertModelInput` simply fails to hold — there
  is no runtime check, the type system refuses.
* **`@@no_unique` gets its own delegate.** Views declared
  `@@no_unique` return a separate `ViewDelegateNoUnique<V>` type
  that omits `find_unique` (and `refresh()`) at the type level, so
  a call like `runtime.views().<v>().find_unique(())` is a compile
  error rather than a runtime `WHERE  = $1` footgun.
* **Migration ordering is automatic.** `cratestack-migrate` lands
  `DROP VIEW` ops before column / table drops the view referenced
  and `CREATE VIEW` ops after the matching column / table adds, so
  body changes that overlap with column changes still apply
  correctly. Body changes are modelled as `Drop + Create` (not
  `CREATE OR REPLACE VIEW`) to preserve that ordering invariant.
* **Policy enforcement is the same machinery models use.**
  `@@allow("read", expr)` lowers into the same `ReadPolicy` array
  consumed by `push_scoped_conditions`. Only the `"read"` action
  is accepted; any other action is a parse error.

Landed end-to-end across eight PRs:
[#84](https://github.com/cratestack/cratestack/pull/84) (parser + IR +
validator),
[#85](https://github.com/cratestack/cratestack/pull/85) (`ReadSource`
/ `WriteSource` traits + `ViewDescriptor`),
[#86](https://github.com/cratestack/cratestack/pull/86) (polymorphic
read helpers),
[#87](https://github.com/cratestack/cratestack/pull/87) (generic
read builders + `ViewDelegate`),
[#88](https://github.com/cratestack/cratestack/pull/88) (macro
emission + `runtime.views()` accessor),
[#89](https://github.com/cratestack/cratestack/pull/89) (migrate IR +
diff + per-backend DDL),
[#90](https://github.com/cratestack/cratestack/pull/90) (policy
lowering),
[#91](https://github.com/cratestack/cratestack/pull/91) (integration
tests vs real Postgres + SQLite). ADR-0003 is `Accepted` in the docs
repo (`cratestack-docs` [#21](https://github.com/cratestack/cratestack-docs/pull/21)).

### Cleanup

* `cratestack-macros` no longer emits selection / projection helpers
  behind a `cfg(not(target_arch = "wasm32"))` gate — `ProjectionDecoder`
  now lives in `cratestack-core` and works on every target.
* The umbrella's banking / policy / migrations / isolation /
  validation / generated-client integration tests are now under
  `crates/cratestack-pg/tests/`; the SQLite e2e test under
  `crates/cratestack-sqlite/tests/`. No test logic was changed.

### Other fixes

* Projected-query decoding now tolerates a missing optional field instead
  of erroring, matching how a partial `SELECT` projection is actually
  expected to behave (#93).
* `codec-json` is now an opt-out feature on `cratestack-client-rust`
  rather than always-on (#94).
* CI's rustdoc build now points at `cratestack-pg`, the facade split's
  replacement for the removed `cratestack` umbrella (#95), and the
  release workflow gained a test-retry + `SKIP_TESTS` escape hatch for
  known-flaky suites (#81).

## 0.3.7 (2026-05-18)

No code changes beyond the version bump itself.

## 0.3.6 (2026-05-18)

Release tooling: publish order is now computed from `cargo metadata`'s
real dependency graph instead of a hand-maintained list, so a new crate
gets the right publish position automatically instead of needing a
manual list edit every time (#80).

## 0.3.5 (2026-05-18)

Release tooling: `release-publish` is now idempotent and resumable — a
partial failure partway through publishing the workspace can be re-run
and picks up where it left off instead of re-attempting crates that
already published successfully (#79).

## 0.3.4 (2026-05-17)

Studio's `eject` command is redesigned from a UI-fork-only tool into a
full-project starter scaffold: `cratestack studio eject --out <dir>`
now writes a runnable binary crate (`Cargo.toml`, `src/main.rs`,
`studio.toml`, an example schema) with the Leptos UI already bundled
in; `--with-ui` additionally unpacks the UI's Trunk sources for
front-end customization. The UI itself moves to a sibling
`crates/cratestack-studio/ui/` crate, embedded into the release binary
as a tarball rather than generated from templates, and
`cratestack-studio-generator` folds into `cratestack-studio` (#78).

## 0.3.3 (2026-05-17)

### Studio rewrite — Phase 1d + 4 (typed editors + power tools)

The final phases of the Studio rewrite. Phase 1d retires the
one-text-box-per-field approach in the create + edit forms; Phase 4
ships SQL preview, drift detection, CSV/JSON export, schema search,
an audit log, and constraint-aware error mapping.

**Typed editors (Phase 1d).** The create form and the drawer's edit
mode now dispatch on each field's declared scalar:

- `<select>` for enums (variants pulled from the schema)
- `<textarea>` for `Json` (free-form, parsed on submit)
- `<input type="datetime-local">` for `DateTime` (auto-normalized to
  `YYYY-MM-DDTHH:MM:SSZ` before the request)
- `<input type="number" step="any">` for `Float` / `Decimal`
- `<input type="number" step="1">` for `Int`
- `<select>` (true/false) for `Boolean`
- plain text for `String`, `Cuid`, `Uuid`, `Bytes`

The `/api/targets/:key/models` response gains `is_enum` and
`enum_variants` per field so the UI doesn't need a second round-trip
to populate the dropdown.

**SQL preview (Phase 4).**

```
GET /api/targets/:key/models/:model/sql?op=list|get|create|update|delete&pk=…
```

Returns the SQL Studio would run plus an ordered parameter list:

```json
{
  "driver": "postgres",
  "sql": "WITH inserted AS ( INSERT INTO \"posts\" …",
  "params": [ { "index": 1, "binding": "title", "kind": "text" }, … ]
}
```

API-backed targets return **501 UNSUPPORTED** — Studio doesn't render
SQL it doesn't run.

**Drift indicator (Phase 4).**

```
GET /api/targets/:key/drift
```

Compares declared columns (from the `.cstack` schema) against the live
database. Each model carries one of: `ok`, `drift` (column mismatch),
`missing_table` (table absent), `unsupported` (API-only target), or
`skipped` (no @id or unsupported PK type). The UI renders an amber
`⚠ drift` badge in the sidebar next to any model that doesn't match,
and a red `✕ table` badge for missing tables.

**CSV/JSON export (Phase 4).**

```
GET /api/targets/:key/models/:model/export?format=csv|json&limit=N
```

Streams up to `EXPORT_CAP = 10_000` rows through cursor pagination
under the hood and returns one body. Sets `Content-Disposition:
attachment; filename="<target>-<table>.<ext>"` so browsers download
the file. CSV uses RFC-4180-style escaping (quote-wrap on commas,
quotes, or newlines; double up embedded quotes).

**Schema search (Phase 4).**

```
GET /api/targets/:key/search?q=<term>
```

Case-insensitive substring over models, fields, enums (and variants),
types, mixins, procedures. Hits return `kind`, optional `model`,
`name`, and a short `detail` so the dropdown can present them. The
search bar in the header debounces on input and shows the dropdown
inline.

**Audit log (Phase 4).** Every successful write (CREATE / UPDATE /
DELETE) is appended to an in-memory ring buffer (cap **500**, FIFO
when full) attached to the workspace. The `Audit` button in the
header opens an overlay listing the most recent entries:

```
GET /api/audit?limit=N
```

Returns newest-first. Entries carry `id`, `at` (RFC-3339), `target`,
`model`, `op`, and the row's `pk` (for CREATE, the post-insert value
the DB filled in).

**SQLSTATE → VALIDATION_ERROR mapping (Phase 4).** Constraint
failures from the driver are now mapped into the same per-field
`VALIDATION_ERROR` envelope the in-process validators produce, so the
UI can drop the message next to the input that broke:

| Source                       | Code           |
| ---------------------------- | -------------- |
| Postgres `23505` / SQLite `SQLITE_CONSTRAINT_UNIQUE` / `…_PRIMARYKEY` | `UNIQUE`       |
| Postgres `23503` / SQLite `SQLITE_CONSTRAINT_FOREIGNKEY`             | `FOREIGN_KEY`  |
| Postgres `23502` / SQLite `SQLITE_CONSTRAINT_NOTNULL`                | `REQUIRED`     |
| Postgres `22001` (string truncation)                                 | `LENGTH`       |
| Postgres `22P02` (invalid text representation)                       | `TYPE_MISMATCH`|
| Postgres `23514` / SQLite `SQLITE_CONSTRAINT_CHECK`                  | `REGEX`        |

Unrecognized driver errors still surface as `DATABASE_ERROR` (500).

**Validation codes.** Two new codes on top of Phase 3:

- `UNIQUE` — unique-constraint violation from the database.
- `FOREIGN_KEY` — foreign-key violation from the database.

**UI surfaces (Phase 4).**

- **Tools row.** Above the records table: an op selector + "Show SQL"
  button that fetches the preview and renders it as monospace SQL +
  bind list. Next to it: "Export JSON" / "Export CSV" links that
  point straight at the export endpoint so the browser handles the
  download.
- **Drift dots.** Each model in the sidebar carries a small status
  chip when its live shape doesn't match the schema.
- **Search.** The header's search input fans out to
  `/api/targets/:key/search` on every keystroke; results render in a
  dropdown below the input.
- **Audit overlay.** "Audit" button next to the target switcher
  toggles a 28rem-wide overlay listing recent writes by timestamp.

**Scope notes.**

- Audit log is in-memory only by design — Studio is a local admin
  tool. Restarting the binary clears the buffer.
- Drift inspection talks to `information_schema` (Postgres) and
  `PRAGMA table_info` (SQLite). API-backed targets are reported as
  `unsupported`.
- Export is bounded at 10_000 rows. Larger pulls should use the
  underlying database directly.

### Studio rewrite — Phase 1c + 3 (UI polish + write path)

Studio gains create / update / delete and the UI polish that goes
with it.

**Write API.** Three new endpoints:

```
POST   /api/targets/:key/models/:model/records          -> 201 + row
PATCH  /api/targets/:key/models/:model/records/:pk      -> 200 + row
DELETE /api/targets/:key/models/:model/records/:pk      -> 200 + row
```

All three reject requests against `mode = "ro"` targets with **403
FORBIDDEN**. Writes are wired on all three data sources: Postgres
uses `INSERT/UPDATE/DELETE … RETURNING *` wrapped in `row_to_json` for
type-blind projection; SQLite mirrors the shape with `RETURNING
json_object(...)`; the API source POSTs/PATCHes/DELETEs to the
upstream service's generated `/api/<plural-snake-model>` routes.

The Postgres write path binds typed values based on the field's
declared scalar — `String`/`Uuid`/`Cuid`/`Decimal`/`DateTime`/`Bytes`
as text, `Int` as `i64`, `Float` as `f64`, `Boolean` as `bool`, `Json`
through `sqlx::types::Json`. Anything else (enums) binds as text and
relies on the DB's enum cast.

**Validator pass-through.** A new `validators` module mirrors the
framework's macro-side validators (`@email`, `@length(min:, max:)`,
`@range(min:, max:)`, `@regex("...")`, `@uri`, `@iso4217`) against the
incoming JSON payload before Studio hits the database. Failures
surface as **422 VALIDATION_ERROR** with a structured per-field detail
list the UI can render inline:

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "payload failed validation",
    "fields": [
      { "field": "title", "code": "LENGTH", "message": "field 'title' must be at least 3 characters long" },
      { "field": "authorEmail", "code": "EMAIL", "message": "field 'authorEmail' is not a valid email address" }
    ]
  }
}
```

Validation codes (all `SCREAMING_SNAKE_CASE`): `REQUIRED`,
`TYPE_MISMATCH`, `EMAIL`, `LENGTH`, `RANGE`, `REGEX`, `URI`, `ISO4217`.
The error envelope adds a `fields: []` array — omitted entirely on
non-validation errors so the existing error contract is unchanged.

**UI updates (Phase 1c + 3).**

- **Typed relation picker.** Drawer's relation follow swaps the free
  text input for a dropdown built from the model's `is_relation`
  fields. Labels show `<field> → <target> (<arity>)`.
- **RO / RW badge.** Each model header now displays a small badge
  reflecting the target's mode, so users see at a glance whether
  edits are allowed.
- **Create flow.** RW targets expose a `+ New` button above the
  records table that opens an inline form with one input per writable
  field. Validation errors surface per-field inline; on success the
  table reloads.
- **Edit flow.** RW targets expose an **Edit** button in the drawer
  that turns the field list into editable inputs. **Save** PATCHes
  the row; the response replaces the drawer's view. Per-field
  validation errors appear inline.
- **Delete flow.** RW targets expose a **Delete** button in the
  drawer guarded by a `window.confirm()` prompt. On success the
  drawer clears and the table reloads.
- **Pretty JSON viewer.** Object/array cell values in the drawer now
  render through `serde_json::to_string_pretty`.

**Error codes.** Two additions on top of Phase 1b's set:

- `FORBIDDEN` (403) — target is read-only.
- `VALIDATION_ERROR` (422) — payload-level validation failure with
  per-field detail.

The earlier (`BAD_REQUEST`) code is now reserved for malformed request
bodies (e.g. invalid JSON); validation errors get their own code so
the UI can route them into per-field error displays.

#### Scope notes

- Validators run before the DB. Constraint-level failures (UNIQUE,
  NOT NULL, CHECK, type mismatch beyond what we catch) still surface
  as `500 DATABASE_ERROR` with the underlying driver message; mapping
  SQLSTATE / SQLite extended codes to friendlier validation
  envelopes is Phase 4.
- The UI's create / edit form is a single text-input per field; typed
  pickers for enums and rich editors for JSON / DateTime / Decimal
  are Phase 1d.
- API targets accept writes and forward them to the upstream's REST
  routes verbatim. The upstream's own policy/auth enforces what's
  actually allowed.

### Studio rewrite — Phase 2 (`studio eject` + bundled UI)

Two things land in this phase. Both are about making Studio
distributable rather than dev-only.

**`cratestack studio eject --out <dir>`** writes a writable copy of
Studio's Leptos+Trunk UI into the target directory: `Cargo.toml`,
`Trunk.toml`, `index.html`, `src/{lib,api,app,types}.rs`, and a
purpose-built `README.md` that explains the standalone build flow.
Generated artifacts (`dist/`, `target/`, `Cargo.lock`) are skipped so
the eject output is a clean checkout. The UI tree is embedded into the
framework binary at compile time via `include_dir!`, so eject is a
single-step copy with no template substitution to drift.

```
cratestack studio eject --out ./fork
# wrote 9 files; cd ./fork && trunk serve
```

`--force` lets you overwrite an existing non-empty directory; without
it, eject refuses to clobber.

**`embed-ui` cargo feature** bundles the Trunk release build into the
Studio binary via `rust-embed`. Build flow:

```bash
cd crates/cratestack-studio/ui && trunk build --release
cargo build -p cratestack-cli --bin cratestack \
  --features cratestack-studio/embed-ui
```

With the feature on, `cratestack studio run` serves the SPA at `/`,
keeps the JSON API mounted at `/api/*`, and falls back to `index.html`
for unknown paths so the browser's client-side routing works. With
the feature off (the default), `/` still serves the Phase 1b stub
explainer so the binary stays minimal for dev.

Wiring: API routes are mounted before the UI routes, so any future
overlap resolves in favor of the JSON surface. The bundled-UI tests
explicitly assert that `/api/targets` still hits the JSON handler
when the SPA fallback is in play.

#### Crate / module changes

- `cratestack-studio` gains `mod eject` (with `eject()`, `EjectOptions`, `EjectError`, `EjectReport`) and an `embed-ui`-gated `mod ui_assets`.
- `cratestack-studio-generator` is now a thin re-export of `cratestack_studio::eject` so the CLI's existing import surface keeps working. New code should depend on `cratestack-studio` directly.
- `cratestack-cli`'s `studio eject` subcommand gains `--force` and now prints the eject report (file count + next-steps hint).
- New workspace deps: `include_dir = "0.7"`, `rust-embed = "8"` (used only when the `embed-ui` feature is on).

#### Scope notes

- The `embed-ui` feature requires a Trunk release build to have produced `crates/cratestack-studio/ui/dist/`. Building the feature without that tree fails fast at the embed step.
- Eject's output README points users at the framework's docs for upstream upgrades. There's no automated re-eject path — a forked UI is a fork.

### Studio rewrite — Phase 1b (read API completions + Leptos UI)

Phase 1b finishes the read story. SQLite targets are now a first-class
driver, the `@relation` traversal endpoint is wired, the API-backed
list/get path talks to deployed CrateStack services, and a Leptos+Trunk
web UI consumes all of it from the browser.

**SQLite via rusqlite.** A new `data::sqlite::SqliteSource` opens a
SQLite connection per target and projects rows through SQLite's
`json_object(...)` so the rest of the pipeline stays identical to the
Postgres path. Studio doesn't use `sqlx-sqlite` because the workspace's
`rusqlite 0.39 → libsqlite3-sys 0.37` pin conflicts with sqlx-sqlite's;
the rusqlite-based source has no such conflict. `[target.db]` URLs
accept `sqlite:`, `sqlite://`, `sqlite::memory:`, and bare file paths.

**Relation follow.** New endpoint
`GET /api/targets/:key/models/:m/records/:pk/rel/:field`. The
resolver reads `@relation(fields: [...], references: [...])` symmetrically
on both ends of a relation: the target is the field's declared type,
the source row's `fields[0]` supplies the bound value, and we filter
the target table on `references[0]`. List-arity fields return a paginated
page; Required-arity fields return a single optional row. Both sides
of the relation must declare `@relation` (which is what the CrateStack
parser already enforces).

**API list/get.** `data::api::ApiSource` now talks to a deployed
CrateStack service over the same REST routes the generated TypeScript
and Dart clients use: `GET <base>/api/<plural-snake-model>` for list,
`GET <base>/api/<plural-snake-model>/{id}` for find_unique. Studio
maps its cursor abstraction onto the upstream's offset/limit pagination
by encoding the next offset as the opaque cursor string. Auth headers
follow `[target.api.auth]` (`bearer { token = … }` or `header { name,
value }`). Relation follow against API targets returns `UNSUPPORTED` —
the generated REST surface doesn't expose arbitrary column filters.

**Dev CORS.** `[workspace] cors_dev = true` (the default) layers a
permissive CORS layer on the router so a Trunk dev server on
`localhost:8080` can talk to the Studio backend on `localhost:7878`.
Set `cors_dev = false` when binding to a wider interface.

**Leptos UI.** New `crates/cratestack-studio/ui/` crate — a Leptos
CSR app built by Trunk, intentionally excluded from the workspace so
`cargo check --workspace` doesn't pull in the `wasm32-unknown-unknown`
toolchain. Surface:

- Header with workspace name and target switcher (shows mode + db/api capability).
- Left sidebar listing the selected target's models.
- Records table with cursor-based pagination (previous/next).
- Record drawer with a per-field view, a relation-follow input, and a
  "Copy Rust query" button that writes the find_unique snippet to the
  system clipboard.

Run locally with `cratestack studio run` in one terminal and
`trunk serve` in `crates/cratestack-studio/ui/` in another; Trunk's
proxy forwards `/api/*` to the backend on port 7878.

**Error envelope additions.** Two new stable codes: `UNKNOWN_FIELD`
(unknown field name on relation follow, 404) and `NOT_A_RELATION`
(field exists but isn't a relation, 400). `INTERNAL_ERROR` is reserved
for blocking-task panics during the SQLite path.

#### Scope notes

- Relation follow is read-only and supports the two common shapes
  (outgoing 1-1 / many-1, inbound 1-many). Many-to-many through a
  junction table returns `UNSUPPORTED`.
- The UI's relation follow currently takes the field name as a free
  text input — a typed dropdown lands in Phase 1c once the UI threads
  the per-model relation field list down to the drawer.
- The Studio binary still ships without the UI compiled in. Phase 2's
  `studio eject` writes the UI's sources to a writable workspace; Phase
  2 / 3 also adds the `rust-embed` bundle for single-binary distribution.

### Studio rewrite — Phase 1a (read API)

The studio gains a real backend. `cratestack studio run` now parses
each target's `.cstack`, opens a sqlx Postgres pool (when the target
has a `[target.db]` block), and serves six read endpoints:

```
GET /api/targets
GET /api/targets/:key/schema
GET /api/targets/:key/models
GET /api/targets/:key/models/:model/records?cursor=…&limit=…
GET /api/targets/:key/models/:model/records/:pk
GET /api/targets/:key/models/:model/snippet?pk=…
```

`/snippet` returns a Rust `find_unique` call against the macro
delegate so you can paste it into a service crate. Primary-key
literals are typed: `String`/`Cuid`/`Uuid`/`Decimal` IDs render as
`"…".to_owned()`, `Int` IDs as `42_i64`.

Pagination is cursor-based on the model's `@id`. Cursors are bound as
text and cast in SQL (`$1::bigint` for Int PKs, no cast for text-shaped
PKs) so the Rust side stays blind to column types. Row projection uses
Postgres's `row_to_json(t.*)` over the model's scalar columns, which
keeps the dynamic decode path off the type-OID treadmill.

Studio now reads `env:NAME` and `file:PATH` references in
`studio.toml`. `target.db.url` and `target.api.auth.{token,value}` are
resolved at boot; unset env vars and missing files surface a load-time
error that names the bad config field.

API responses use a uniform error envelope —
`{"error": {"code": "…", "message": "…"}}` — with stable codes
(`UNKNOWN_TARGET`, `UNKNOWN_MODEL`, `NO_PRIMARY_KEY`,
`INVALID_PRIMARY_KEY`, `UNSUPPORTED`, `DATABASE_ERROR`,
`UPSTREAM_ERROR`).

#### Scope limits

- **Postgres only.** The workspace currently pins `rusqlite` (used by
  `cratestack-rusqlite` and `cratestack-client-store-sqlite`) against
  `libsqlite3-sys` 0.37, which conflicts with `sqlx-sqlite`'s pin.
  Phase 1b adds an alternate SQLite path that uses `rusqlite` directly
  so the two crates can coexist.
- **No relation follow yet.** `/api/targets/:key/models/:m/records/:pk/rel/:f`
  lands in Phase 1b alongside the UI.
- **API-only targets return 501 on list/get.** Schema and snippet
  endpoints work because they read the parsed schema, not the upstream;
  list/get against `[target.api]` targets is wired in Phase 1b.
- **Primary-key types.** Phase 1a accepts `String`, `Cuid`, `Uuid`,
  `Decimal`, and `Int`. Other PK types (`DateTime`, `Bytes`, etc.)
  return `UNSUPPORTED`.

### Studio rewrite — Phase 0 (breaking)

The Jinja-templated `cratestack generate-studio` scaffold is removed. In its
place is a new crate, `cratestack-studio`, and a new CLI surface,
`cratestack studio`, with three subcommands:

```sh
cratestack studio init                  # writes ./studio.toml
cratestack studio run                   # binds 127.0.0.1:7878 by default
cratestack studio eject --out ./out     # Phase 2 — currently returns NotImplemented
```

The studio now reads a workspace file (`studio.toml`) that lists one or
more `[[target]]` blocks. Each target points at a `.cstack` schema and
declares how the studio reaches its data: a `[target.db]` block for
direct sqlx connections, a `[target.api]` block for a deployed
cratestack service, or both. A target with neither channel is rejected
at load time.

Phase 0 only ships the skeleton: config loader, target validation, and
an Axum server that exposes `/` (stub page) and `/api/health` (workspace
+ target summary). Schema introspection, record browsing, mutations, and
the Leptos UI follow in Phases 1-4.

`cratestack-studio-generator` is now a transitional shim. Its 0.3.x
public API (`generate_package`, `StudioGeneratorConfig`,
`StudioGeneratorContext`, `StudioProfile`, `GeneratedStudioFile`,
`GeneratedStudioPackage`) is gone; the only remaining surface is a
placeholder `eject()` that will, in Phase 2, copy `cratestack-studio`'s
own sources into an output directory for users who want to fork the UI.

Migration for existing `generate-studio` users: run `cratestack studio
init` to seed a `studio.toml`, fill in your schemas and connection
strings, then `cratestack studio run`. There is no automated migration
of the 0.3.x multi-crate output — it was generated code and should be
regenerated from the new shape.

### RPC transport (v1): `transport rpc` as an alternative to REST

A `.cstack` schema now picks exactly one generation style via a
top-level `transport rest|rpc` directive (default `rest`, so existing
schemas parse unchanged) — one binding's worth of public surface, not
both. Under `transport rpc`, every CRUD verb per model and every
procedure gets an op id (`model.User.list`, `procedure.publishPost`),
dispatched over two endpoints instead of a route per model/verb:

```
POST /rpc/:op_id       # unary
POST /rpc/batch        # server may parallelize; no in-batch dependencies,
                        # no transactional mode — use a procedure or two
                        # round trips for composite ops
```

The op id lives in the URL rather than the request body — operationally
honest, since nginx/CDN/HTTP tracing all work per-route that way — and
client codegen branches on the schema's transport style, so a generated
SDK ships one client's worth of code, not both (#20–#24, examples in
#27). Error responses use gRPC-style codes in a stable `RpcErrorBody`
shape (#23). Streaming (`application/cbor-seq`) needed no code change
at all: content negotiation on the existing sequence encoder already
handled it (#24).

**Deferred:** the WebSocket binding and `@@subscribe`-driven
subscriptions from the original design are not part of this release —
today's audit/event-bus consumers are server-to-server and don't need
a WS channel, so this is picked up when a concrete consumer needs it,
not before (#25).

### ORM additions

Landed alongside the RPC work above, independent of transport style:

- **Transaction-aware writes**: `.for_update()` and `update_many` join
  the existing write surface, both participating correctly in an
  ambient transaction (#26).
- **Composite-key upsert** and **`find_unique` detail policy** support
  (#28).
- **Nullable-OR filter** and a **`COALESCE` multi-column filter** for
  querying across nullable columns without hand-written SQL (#29).
- **`aggregate`**, **`delete_many`**, and `NULLS FIRST`/`NULLS LAST`
  ordering (#37).
- **JSONB filter operators** — `json_has_key` + `json_get_text` (#42).
- **`FindMany.include()`** — to-one relation side-loading in a single
  round trip (#44).
- **PostGIS spatial filters** — `covers_geography` + `dwithin_geography`
  (#48).
- **Column projection** — `find_*.select(...)` returning a typed
  `Projection<T>` instead of the full model (#51).
- **`ProjectedFindMany.run_in_tx`**, plus an `enum` `Default` fix (#55).

### Client streaming (cbor-seq)

The generated clients gain first-class consumers for the streaming
transport introduced above:

- **Rust**: `RpcClient::call_streaming` returns an `mpsc::Receiver`,
  fed by a `cbor-seq` streaming decoder (#30, #34). Also gains a typed
  batch API — same method, two consumption modes (#53).
- **Dart**: `CborSeqStreamTransformer` + a decoder-handle contract
  (#43), and an `rpc_call_streamed` FFI entrypoint for
  `/rpc/{op_id}` (#39).
- **Flutter**: `execute_streamed` FFI shim over the cbor-seq path
  (#33), and `FlutterCborSeqDecoder` for `dio`-driven streaming (#40).
- **Codegen**: client generators now branch on `Schema.transport` to
  emit RPC clients where the schema calls for them (#32, #50).

### Workspace-wide 200-LoC refactor

Every `.rs` file under `crates/*/src/` is now ≤200 LoC, landed across 16
PRs (#57–#76). No public API changes — all splits preserve the crate
surface via `pub use` re-exports. The major rewrites:

- `cratestack-sqlx` and `cratestack-rusqlite` delegate / render / batch /
  value modules split into focused submodules
- `cratestack-axum` idempotency, rpc, transport, ratelimit, headers,
  codec all broken into per-concern files
- `cratestack-macros` four giants split (include / model / axum /
  relation), medium files re-grouped
- `cratestack-client-{dart,rust,typescript,flutter}` `lib.rs` split into
  per-concern modules (largest: client-rust at 2369 → 18 submodules)
- `cratestack-parser` 880-line `parse.rs`, 1086-line `validate.rs`, and
  1336-line `tests.rs` split per topic
- `cratestack-lsp` `main.rs` (1273 LoC) split into 11 submodules
- `cratestack-client-dart` README and rpc-runtime jinja templates split
  via `{% include %}` fragments (loader sets
  `set_keep_trailing_newline(true)` for byte-identical output)
- Inline `#[cfg(test)] mod tests` blocks throughout the workspace
  extracted into `tests_<topic>.rs` siblings

### README fixups

Four crate READMEs (`cratestack-axum`, `cratestack-sqlx`,
`cratestack-client-rust`, `cratestack-parser`) still referenced the
pre-0.3.0 macro names (`include_schema!`,
`include_client_macro!`) — updated to the current
`include_server_schema!` / `include_client_schema!`. The `client-rust`
README's two duplicate sections (one per old macro) collapse into one.

### Other

Test-support scaffolding (`tests/support/pg.rs`) covering
compose/testcontainers/skip backend selection for PG-backed integration
tests (#19), and an internal `cratestack-axum` module split
(codec/transport/headers/query) with deduped RPC helpers (#31).

## 0.3.2 (2026-05-14)

### Batch primitives — tRPC-style per-item envelope

Five new ORM methods on every model delegate, on both the sqlx (server) and rusqlite (embedded) backends:

```rust
cool.account().batch_get(vec![1, 2, 999]).run(&ctx).await?
cool.account().batch_create(vec![input_a, input_b]).run(&ctx).await?
cool.account().batch_update(vec![(1, patch_a, Some(0)), (2, patch_b, None)]).run(&ctx).await?
cool.account().batch_delete(vec![1, 2]).run(&ctx).await?
cool.account().batch_upsert(vec![input_a, input_b]).run(&ctx).await?
```

Every batch call returns `Result<BatchResponse<M>, CoolError>`. The outer `Result` is reserved for whole-batch infrastructure failures (size cap exceeded, duplicate input keys, DB connection lost). Per-item failures (validation, policy denial, NotFound, stale `if_match`, PK conflict) ride inside the envelope as `BatchItemStatus::Error { error: BatchItemError { code, message } }`, with `index` preserved so callers can pair results back to their input position.

```json
{
  "results": [
    { "index": 0, "status": "ok", "value": { ... } },
    { "index": 1, "status": "error", "error": { "code": "POLICY_DENIED", "message": "..." } },
    { "index": 2, "status": "ok", "value": { ... } }
  ],
  "summary": { "total": 3, "ok": 2, "err": 1 }
}
```

### Transactional model

- **Two single-statement ops** (`batch_get`, `batch_delete`) issue one `SELECT … WHERE pk IN (…)` or `DELETE … WHERE pk IN (…) RETURNING …`. Policy predicates merge into the WHERE; rows that don't match (because they don't exist, were already tombstoned, or the read/delete policy hid them) surface as per-item `NOT_FOUND`.
- **Three savepointed ops** (`batch_create`, `batch_update`, `batch_upsert`) run all items in one outer transaction with a per-item `SAVEPOINT`. A per-item failure rolls back its savepoint only — successful items in the same batch still commit. The audit log records one row per successful item, with the outer commit timestamp; failed items leave no audit row, no event outbox entry, no row mutation.
- The cap is `1000` items per call (`cratestack_core::BATCH_MAX_ITEMS`); over-sized batches are rejected before any SQL runs.

### Loud-fail on duplicate input keys

The framework refuses batches with duplicate primary keys at the outer guard, returning `CoolError::Validation` (or `RusqliteError::DuplicateBatchKey` on the embedded side) with the indices of the first and duplicate occurrences. Silently collapsing duplicates would break the per-item `index` mapping the envelope promises and hide caller bugs; we want callers to dedupe at the boundary they own.

Detection runs on:

- the PK list for `batch_get` / `batch_delete`
- the per-item PK in `batch_update` items
- `UpsertModelInput::primary_key_value()` for `batch_upsert`

`batch_create` skips the check — `CreateModelInput` doesn't expose the PK generically, and duplicate client-supplied PKs already trip the database's unique constraint per-item (surfacing as `CoolError::Conflict` in that item's envelope, while the rest of the batch commits cleanly via savepoint isolation).

### Internal

- New types in `cratestack-core`: `BatchItemResult<T>`, `BatchItemStatus<T>`, `BatchItemError`, `BatchSummary`, `BatchResponse<T>`, `BatchRequest<I>`, `BATCH_MAX_ITEMS`, `find_duplicate_position`.
- New trait in `cratestack-sql`: `ModelPrimaryKey<PK>`, emitted by the macro on every generated model struct. Used by `batch_get` / `batch_delete` to pair returned rows back to their input position.
- New helper in `cratestack-sql`: `find_duplicate_sql_value` for upsert-side dedup, since `SqlValue::Float` / `SqlValue::Decimal` don't admit a sound `Hash` impl.
- New `RusqliteError` variants: `BatchTooLarge { actual, maximum }` and `DuplicateBatchKey { first, duplicate }`.

### Worked example

The `examples/embedded-cli` notes app gains three batch subcommands that walk through the envelope in real terminal output:

```text
$ notes import bulk-load.json
OK  [0] 11111111-…  first
OK  [1] 22222222-…  second
summary: 2 total, 2 ok, 0 err

$ notes bulk-done 11111111-… 99999999-…
OK  [0] 11111111-…  first
ERR [1] NOT_FOUND: no row matched
summary: 2 total, 1 ok, 1 err
```

- `notes import <file.json>` — `batch_upsert` over a JSON file; replays converge.
- `notes bulk-done <id> [id...]` — `batch_update` to mark complete.
- `notes bulk-delete <id> [id...]` — `batch_delete`.

### Deferred

- **Auto-generated `POST /<model>/batch-*` axum routes**: the wire envelope types (`BatchRequest<I>` / `BatchResponse<T>`) are stable in `cratestack-core` so apps can hand-roll a thin handler against the ORM today. Macro-driven route emission per model lands in a follow-up.
- **Per-item `if_match` on the embedded `batch_update`**: the rusqlite layer doesn't enforce `@version` for single rows either; consistency over surprise.

## 0.3.1 (2026-05-14)

### New crate: `cratestack-migrate` — schema diff + migration generator

Implements ADR-0004, the *authoring* side of the migration story: a new
`cratestack-migrate` crate diffs a parsed `.cstack` against a committed
snapshot and emits per-backend SQL migrations. The runner (already in
`cratestack-sqlx`) is unchanged — it consumes the generated SQL
identically to hand-written migrations.

```
cratestack migrate diff --schema schema.cstack --out-dir migrations --backend both --name <slug>
```

Per-backend output lives under
`migrations/<postgres|sqlite>/<timestamp>_<slug>/` as `up.sql` /
`down.sql`, alongside a committed `schema.snapshot.json`. The diff
engine produces a backend-agnostic op list ordered by DDL dependencies
(enums → renames → drops → creates → adds → check constraints → enum
drops), covering table/column add-drop, indexes (from `@unique`),
column type/nullability/default changes, renames (`@@rename` /
`@rename`), enums, and check constraints (`@db_enforce` promotion of
`@range` / `@length` / `@iso4217`).

**Destructiveness gating.** Every op is classified Safe / Lossy /
Blocking; `--allow-destructive` is required to write any migration
containing a lossy op, and `down.sql` for a lossy migration is an
explicit error stub (`RAISE EXCEPTION` / `RAISE(FAIL, ...)`) rather
than a real rollback — matching the runner's irreversible-by-default
posture (#16).

**Deferred (intentional):** `migrate verify` and `migrate drift` need
ephemeral DB spawning and live introspection, each with its own CI
footprint; view-block IR ops need the `view` block itself (ADR-0003)
built out first; `DropEnumVariant` needs a Postgres swap-dance plus a
backfill plan for referencing rows.

### Examples, docs, and CI

- Pure-Rust example set covering all three 0.3.0 macros side by side
  (#10), and a root README rewrite for the macro split (#11).
- In-browser embedded SQLite example plus a wasm32 facade refactor
  (#12); `embedded-expo` × `embedded-flutter` × `tauri-native` (#14);
  `embedded-daemon` + `embedded-webhook` showing async I/O layered
  around the sync `ModelDelegate` (#15).
- CI's rustdoc job now restricts to the framework crates so it doesn't
  pull in GTK transitively via the Tauri examples (#13).

### Upsert primitive

New `.upsert(input)` on every model whose `@id` is client-supplied (i.e. has no `@default(...)`). Backed by `INSERT … ON CONFLICT (<pk>) DO UPDATE …`. Available on both the sqlx (server) and rusqlite (embedded) backends.

```rust
// Server (sqlx) — both create and update policies enforced, event/audit
// driven off a SELECT … FOR UPDATE probe inside the same transaction.
cool.tag().upsert(CreateTagInput { id, label }).run(&ctx).await?;

// Embedded (rusqlite) — single statement, no audit/event machinery.
delegate.upsert(CreateTagInput { id, label }).run()?;
```

Models with server-generated PKs (`@id @default(cuid())`, etc.) get **no** `UpsertModelInput` impl — calling `.upsert(...)` on them is a compile error rather than a runtime "not supported." Unique-key (non-PK) conflict targets are deferred.

Semantics:

- **Both create and update policies must allow the call** — evaluated at call time, before the runtime knows which branch will fire. Pre-flighting a read to pick a policy slot would leak row existence to the caller.
- **`@version` columns are bumped server-side** on the update branch (`<table>.<col> + 1`). `if_match` is not supported on upsert — use `.update(...).if_match(...)` if you need it.
- **Soft-deleted rows act as "no row"**: the INSERT branch will then trip the PK uniqueness constraint, which is the right outcome (refuse to silently revive a tombstone).
- **Event / audit fan-out** picks `Created` vs `Updated` based on whether the `SELECT FOR UPDATE` probe saw a row — not Postgres `xmax`, so the rusqlite mirror stays trivial.
- **Auth-derived defaults (`@default(auth().*)`) are excluded from the update branch** — they're identity bindings, and clobbering them on update would turn upsert into "take ownership of any row I name." The full list of columns the update branch is allowed to overwrite is exposed on `ModelDescriptor::upsert_update_columns`.

### Internal

- `ModelDescriptor::new(...)` gained one trailing argument (`upsert_update_columns`). Schemas built through `include_*_schema!` are unaffected; hand-rolled descriptors need the extra `&[]`.

## 0.3.0 (2026-05-13)

### New crate: `cratestack-rusqlite` — the embedded SQLite backend

The embedded backend's actual implementation: `ddl`, `delegate`,
`render`, `row`, `runtime`, and `value` modules, plus an `ffi` layer for
non-Rust embedders. This is the concrete crate the three-macros split
below routes `include_embedded_schema!` to, and what the wasm/OPFS
capability under "New features" builds on.

### New crate: `cratestack-redis` — idempotency and rate-limit stores

Two server-side Redis-backed stores, siblings to `cratestack-sqlx`'s
equivalents, for multi-replica deployments that need shared state
across instances rather than per-process memory:

- **`RedisIdempotencyStore`** implements
  `cratestack_axum::idempotency::IdempotencyStore`. Atomicity comes from
  three Lua scripts (`reserve_or_fetch`, `complete`, `release`) run via
  `EVALSHA` with `NOSCRIPT` fallback; reservation lifetimes are driven
  by `PEXPIREAT`, and token rotation on reclaim plus token/status guards
  inside `complete`/`release` stop a stale handler from poisoning a
  newer reservation. State lives in one Redis hash per
  `(principal, key)` at `<prefix>:idem:<sha256(principal || 0x00 ||
  key)>` (#5).
- **`RedisRateLimitStore`** enforces a single global token-bucket per
  key across replicas via one atomic read-refill-decrement-write Lua
  script; bucket state lives at `<prefix>:rl:<sha256(key)>` with a
  self-refreshing `EXPIRE` so idle keys evict themselves (#7).

Both skip their live-Redis integration tests cleanly when no Redis is
configured, matching the existing sqlx-store test pattern.

### Headline: three macros, one schema, no dead weight

The single `include_schema!` macro is gone. In its place are three role-specific macros that emit only what each deployment needs. No more mobile apps transitively pulling `sqlx` they don't use; no more server builds carrying `rusqlite` for nothing.

```rust
// Server (Postgres via sqlx) — full ORM, axum routes, procedures, events
include_server_schema!("schema.cstack", db = Postgres);

// Embedded (rusqlite) — works native and on `wasm32-unknown-unknown` via OPFS
include_embedded_schema!("schema.cstack");

// HTTP client — model/input stubs, procedure clients, zero DB
include_client_schema!("schema.cstack");
```

The split is **strict**: `include_server_schema!` does not emit anything rusqlite-related, and `include_embedded_schema!` does not emit anything sqlx-related. Each deployment shape pays only for its own surface.

### Breaking changes

- **Removed `include_schema!`.** Migrate server callers to `include_server_schema!("…", db = Postgres)`. Migrate sqlite/embedded callers to `include_embedded_schema!("…")`.
- **Renamed `include_client_macro!` → `include_client_schema!`** for naming consistency with the new macros.
- **`include_server_schema!` requires a `db = …` argument.** Today only `db = Postgres` is accepted; the parser is wired so adding `MySql` / `Sqlite`-via-sqlx in a future release is non-breaking at call sites that already pass `db = Postgres`.
- **`include_embedded_schema!` emits `::cratestack_rusqlite::*` paths**, not `::cratestack::*`. Embedded consumers should list `cratestack-rusqlite` and `cratestack-macros` directly in their `Cargo.toml`; the heavyweight `cratestack` facade is no longer required for an embedded build.
- **Deleted the `cratestack-sqlite-wasm` crate.** Originally written as a separate wasm32 backend; superseded by `rusqlite 0.39`, which targets wasm32 transparently via `sqlite-wasm-rs`. Use `cratestack-rusqlite` with the `wasm32-unknown-unknown` target and the new `cratestack_rusqlite::opfs::install_opfs_vfs()` helper (must run inside a Dedicated Worker).
- **Bumped `rusqlite` to `0.39`** (from the previously-resolved `0.32`). Internal `u64` columns now require the `fallible_uint` feature (enabled by default in our workspace pin).
- **Internal: `cratestack-sqlx` migrated off the `sqlx` umbrella crate** to depend on `sqlx-core` + `sqlx-postgres` directly. The umbrella's `sqlx-sqlite` leaked into the resolve graph and conflicted with `rusqlite 0.39`'s `libsqlite3-sys 0.37`. Public surface stays as `cratestack::sqlx::*` via a compatibility shim in `cratestack-sqlx` — no consumer changes required for code that referenced the facade path.
- **Internal: `cratestack-lsp` migrated from unmaintained `tower-lsp 0.20` to `tower-lsp-server 0.23`.** The fork ports the same crate to native `async fn` in traits (Rust 1.75+), drops `#[async_trait]` attributes, renames `lsp_types` → `ls_types`, and switches `Url` → `Uri` (from `fluent-uri`). User-facing LSP behavior unchanged.

### Migration cheat sheet

| Before | After |
|---|---|
| `include_schema!("schema.cstack");` (server context) | `include_server_schema!("schema.cstack", db = Postgres);` |
| `include_schema!("schema.cstack");` (sqlite/mobile context) | `include_embedded_schema!("schema.cstack");` |
| `include_client_macro!("schema.cstack");` | `include_client_schema!("schema.cstack");` |
| `use cratestack::include_schema;` | `use cratestack::{include_server_schema, include_embedded_schema, include_client_schema};` (pick what you need) |

### New features

- **In-browser SQLite ORM.** `cratestack-rusqlite` now compiles to `wasm32-unknown-unknown`. The new `cratestack_rusqlite::opfs::install_opfs_vfs(&OpfsOptions::default()).await?` installs the OPFS SAH-pool VFS so `RusqliteRuntime::open(filename)` persists across page reloads. Must run inside a Dedicated Worker.
- **Single SQLite backend everywhere.** The same `cratestack-rusqlite` crate now serves mobile (libsqlite3), desktop (libsqlite3), and browser (OPFS via `sqlite-wasm-rs`). One code path, one API.

### Known follow-ups

- `@@audit` and `@@emit` directives are currently no-ops in `include_embedded_schema!`. The local-journal / local-event-bus implementations need their own design pass (sync engine, conflict resolution); they will land in a follow-up release.
- `cratestack-sqlx` could lose its `cratestack::sqlx::*` compatibility shim once we've validated nobody depends on it externally. Tracked as a 0.4.0 cleanup.
- Multi-DB support (MySQL, SQLite-via-sqlx) for `include_server_schema!` — the `db = …` arg parser is ready; the codegen needs the abstraction.

## 0.1.0

Initial public extraction release.

This release includes the Rust workspace, CLI, parser, macros, codecs, Axum and SQLx integration crates, generated Rust/Dart/TypeScript client support, the `.cstack` language server, and the VS Code extension package.
