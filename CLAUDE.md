# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

CrateStack is a Rust-native, schema-first framework. You write a `.cstack` schema and a compile-time
macro generates the typed Rust surface — models, CRUD routes, procedures, policies, clients — for one
of three deployment roles. The framework is pre-1.0; the public crates are versioned together off the
workspace `version` in the root `Cargo.toml`.

## Commands

Most workflows are encoded in the `justfile` (`just --list`). The important ones:

> **Linux prerequisite:** the workspace includes the `tauri-*` example crates, whose Linux backend pulls
> `glib-sys`/`webkit2gtk-sys` — so a fresh `--workspace` build/test on Linux needs the GTK/WebKit dev
> packages installed (`libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, …). macOS uses the system WebKit and needs
> nothing extra. The rustdoc CI job sidesteps this by building the framework crates by name, not `--workspace`.

- **Pre-PR gate:** `just all-checks` — runs `cargo fmt`, `cargo fix`, `cargo clippy --fix -D warnings`,
  `cargo check --all-targets`, and `cargo deny check`, all scoped `--workspace --exclude
  embedded_flutter_native`. This is the canonical formatting + lint pass; run it before opening a PR.
  (Deliberately **not** `--all-features` — see the plain-tests note below.)
- **Build:** `cargo build --workspace --exclude embedded_flutter_native` (the Flutter native crate needs
  flutter_rust_bridge-generated glue that isn't checked in — see the test note below).
- **Plain tests (no DB):** `cargo test --workspace --exclude embedded_flutter_native`. PG-backed
  integration tests (`banking_*`, `policy_db_*`, `generated_client_rust`) **skip silently** when
  `CRATESTACK_TEST_DATABASE_URL` is unset — a green run here does *not* mean full coverage.
  Two flags to avoid: `embedded_flutter_native` needs flutter_rust_bridge-generated glue that isn't
  checked in (hence the `--exclude`, mirroring the `just` recipes), and `--all-features` enables both
  mutually-exclusive `decimal-*` backends, which trips a `compile_error!` in `cratestack-core`.
- **PG-backed tests:** `just test-pg` — brings up the Postgres container from `compose.yml` (port `55432`),
  runs the full suite, and tears the container down on exit even on failure. `just test-pg-only` is the
  faster inner loop (server facade only). `just test-pg-tc` uses ephemeral per-binary testcontainers
  (what CI uses; stronger isolation, per-binary spin-up cost).
- **Single test:** `cargo test -p <crate> <test_name>`, or under PG:
  `just test-pg-only -- <test_name>` (extra args pass through to `cargo test`). Note: `just test-pg`
  hardcodes `--workspace`, which conflicts with `-p`, so use `test-pg-only` to scope to one crate.
- **Release validation:** `just release-check` (check + tests, retried 3× to absorb the known-flaky
  `generated_routes_emit_tracing_events`; `SKIP_TESTS=1` overrides).
- **Version bump:** `just bump 0.x.y` rewrites every `Cargo.toml` version literal and refreshes the lock.
- **Release:** `just release 0.x.y` (bump → validate → publish in topo order → tag; `PUSH=1` to push).
  Do not hand-maintain publish order — it is topo-sorted from `cargo metadata` at recipe time.
- **CLI:** `cargo run -p cratestack-cli -- <check|generate-dart|generate-typescript|studio|migrate|init|run|eject|diff|print-ir>`

### Critical test gotcha

`-p cratestack` selects an **empty documentation-only vitrine crate**, not a real package — it will
return a false green. Always target `-p cratestack-pg` (server facade) or `-p cratestack-sqlite`
(embedded facade) explicitly. Likewise `embedded_flutter_native` is excluded from workspace test runs
(`--exclude embedded_flutter_native`) because of flutter_rust_bridge toolchain requirements.

## Architecture

### The three-macro / role model (the central idea)

One `.cstack` schema, three mutually-exclusive entry macros — **pick one per consuming crate** based on
what that crate *is*:

- `include_server_schema!("schema.cstack", db = Postgres)` — sqlx + axum + procedures + events; owns a
  Postgres DB. (`db = Postgres` is currently the only value; the parser is wired so future backends are
  non-breaking at existing call sites.)
- `include_embedded_schema!("schema.cstack")` — rusqlite only, sync, **no policy enforcement**. Compiles
  to native *and* `wasm32-unknown-unknown` (browser/OPFS via `sqlite-wasm-rs`) from the same source.
- `include_client_schema!("schema.cstack")` — HTTP client stubs only; treats another service's schema as
  a contract, owns no DB.

### Two disjoint facades

As of 0.4.0 the umbrella crate is split into two facades consumers select via Cargo's `package =` rename:

- `cratestack = { package = "cratestack-pg" }` — Postgres + Axum + Rust client runtime. Does **not** pull
  `libsqlite3-sys`, so it coexists with the official `sqlx` umbrella without `links = "sqlite3"` clashes.
- `cratestack = { package = "cratestack-sqlite" }` — rusqlite (native + wasm) + shared surface.

**Hard rule (enforced by convention, watch for regressions):** the macro split must stay strictly
disjoint. `include_server_schema!` emits sqlx-only code; `include_embedded_schema!` emits rusqlite-only
code. No cross-backend impls leak between the two paths.

### Crate layering

The dependency flow is roughly: **parser → core/policy/sql → macros → backend runtimes / clients**.

- `cratestack-parser` — `.cstack` parser + semantic checker (chumsky-based).
- `cratestack-core` — shared metadata, auth context, codec, error/envelope types, transport descriptors.
- `cratestack-policy` — canonical policy literals, predicates, procedure-policy evaluation.
- `cratestack-sql` — dialect-agnostic SQL primitives shared by both backends.
- `cratestack-macros` — **the codegen heart.** All compile-time generation lives here, organized by
  concern: `include/` (the three entry macros + server collectors), `model/`, `procedure/`, `view/`,
  `relation/`, `policy/`, `transport/` (REST vs RPC dispatch), `axum/`, `client/` (rust/dart/ts, rest/rpc).
- Backend runtimes: `cratestack-sqlx` (Postgres), `cratestack-rusqlite` (embedded), `cratestack-axum`
  (route integration), `cratestack-redis` (server idempotency/rate-limit stores).
- Clients: `cratestack-client-rust`, `-dart`, `-typescript`, `-flutter`, plus client state stores
  (`-store-sqlite`, `-store-redis`).
- Codecs: `cratestack-codec-cbor` (default wire format), `cratestack-codec-json`.
- Tooling: `cratestack-cli`, `cratestack-lsp` (tower-lsp-server LSP for `.cstack`), `cratestack-migrate`,
  `cratestack-studio` (+ `-studio-generator` shim, `-studio-ui` wasm app — see below).

### Transport: REST vs RPC

A schema declares either REST routes (default) or `transport rpc`. The two are mutually exclusive per
schema. RPC collapses the surface to two endpoints — `POST /rpc/{op_id}` (unary) and `POST /rpc/batch` —
dispatched by a generated string `match` on dotted op IDs (`model.<Model>.<verb>`, `procedure.<name>`).
Spec lives in `docs/design/rpc-transport.md`; generation is under `cratestack-macros/src/transport/` and
`include/server/rpc_module/`.

### Studio UI build

`cratestack-studio-ui` is a Trunk-built `wasm32` app, **excluded** from the workspace (`exclude` in root
`Cargo.toml`) so developers aren't forced onto the wasm toolchain. It's bundled into the served binary via
`just bundle-studio-ui` (requires `trunk` + the `wasm32-unknown-unknown` target) and shipped as gitignored
tarballs that `cargo publish` includes explicitly. `just publish-studio` re-bundles before publishing.

## Conventions

- `unsafe_code = "forbid"` workspace-wide.
- Rust source uses `snake_case` filenames (rustfmt convention); all other files are `kebab-case`.
- **200-LoC file ceiling:** there is an active, validated convention of keeping each source file under
  ~200 lines, splitting larger files by concern (this is why `macros/` and `axum/` are deeply nested).
  When adding code, prefer extending the existing fine-grained module layout over growing a file past
  the threshold. Refactor PRs are scoped per-crate.
- Don't commit generated build output, local DB state, or the studio tarballs (gitignored by design).

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
