# Changelog

## 0.3.2 (unreleased)

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

## 0.3.1 (unreleased)

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

## 0.3.0 (unreleased)

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
