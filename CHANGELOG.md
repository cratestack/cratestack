# Changelog

## 0.3.2 (unreleased)

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
