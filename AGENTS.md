# CrateStack Agent Instructions

## Project Summary

Rust workspace for a schema-first framework (`.cstack` files). Three deployment roles via schema macros: `include_server_schema!` (Postgres + Axum), `include_embedded_schema!` (SQLite native + wasm32), `include_client_schema!` (HTTP client).

Two main facade crates, both publish as lib named `cratestack`:
- `cratestack = { package = "cratestack-pg" }` — backend services
- `cratestack = { package = "cratestack-sqlite" }` — embedded/mobile

## Core Commands

```sh
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features            # skips PG integration when no DB URL
just test-pg                                     # full suite with Docker Postgres
just test-pg-tc                                  # same via testcontainers
```

`just all-checks` runs fmt, auto-fix, clippy with `-D warnings`, check, and `deny check`.

## Testing Nuances

- PG integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) require either:
  - `just test-pg` — brings up `compose.yml` Postgres, tears down on exit
  - `CRATESTACK_TEST_DATABASE_URL=...` + manual `docker compose up -d postgres`
  - `just test-pg-tc` — ephemeral containers per test binary (CI default)
- `embedded_flutter_native` is excluded from `--workspace` tests because `flutter_rust_bridge`'s cargokit requires the crate name to use underscores

## Workspace Structure

- `crates/cratestack-*` — framework libraries
- `examples/*` — runnable examples (see `examples/README.md`)
- `packages/cratestack-vscode` — VS Code extension (pnpm)
- `crates/cratestack-studio-ui` — **excluded** from workspace to avoid forcing wasm32 toolchain on all devs

## Lints

- `unsafe_code = "forbid"` (workspace-level)
- Clippy runs with `-D warnings` in `just all-checks`

## Wasm Build Requirements

`cratestack-rusqlite` builds to `wasm32-unknown-unknown` but needs a wasm-capable clang:

- **macOS**: `brew install llvm && export CC=/opt/homebrew/opt/llvm/bin/clang`
- **Ubuntu/Debian**: `sudo apt-get install clang lld`

Apple Xcode clang lacks the wasm32 backend.

## VS Code Extension

```sh
cd packages/cratestack-vscode
pnpm install
pnpm run test:smoke
pnpm run package:vsix      # stages server binary first
```

## Version Bumping

Use `just bump NEW_VERSION` — it rewrites every `Cargo.toml` in the repo and refreshes `Cargo.lock`.

## Release

`just release VERSION` handles the full flow including topo-sorted crate publish order. See `RELEASE.md` for details.

## Code Conventions

- Files: `kebab-case` (Rust: `snake_case` per `rustfmt`)
- Public types: `PascalCase`
- `Cargo.toml` edition: `2024`

## Schema Macros

- `include_server_schema!("path.cstack", db = Postgres)` — sqlx + axum + generated routes + auth + events
- `include_embedded_schema!("path.cstack")` — rusqlite (sync, no tokio, no policies), compiles to native + wasm32
- `include_client_schema!("path.cstack")` — generated client types + reqwest runtime, no DB

## Key Patterns

- `[lib] name = "cratestack"` in `cratestack-pg` and `cratestack-sqlite` allows integration tests to resolve `::cratestack::*` paths directly
- `@@materialized` views only on server backend — macro hard-errors on embedded
- Policies (`@@allow`/`@@deny`) are enforced server-side only; embedded backend doesn't gate access
<!-- BEGIN: AI Governance stanza (managed by ADORSYS-GIS/ai-governance) -->
## AI Governance

AI may accelerate the work, but humans own intent, verification, and consequences.
AI output is not truth: review AI-generated code as untrusted, and never submit work you cannot explain.

When opening issues or pull requests in this repo:

- Use the provided **issue forms** (Epic, User Story, Dev Ticket) and the **pull request template** — do not open blank issues/PRs.
- Fill in the **AI Usage Declaration** honestly (what AI was used for, what you verified).
- Include a **source-of-truth link** (a URL or `#123` reference). No source of truth means the work is not ready.
- Provide **verification evidence** (commands, logs, links, or checked verification boxes). No evidence means it is not done.

Source of truth and full doctrine: https://adorsys-gis.github.io/ai-governance/
This stanza is intentionally thin — read the site; do not duplicate the doctrine here.
<!-- END: AI Governance stanza -->
