# Changelog

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
