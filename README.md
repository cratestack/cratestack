# CrateStack

CrateStack is a Rust-native, schema-first framework workspace for building typed HTTP APIs, generated clients, and backend services from `.cstack` files.

The implementation is still pre-1.0. As of `0.3.0` the framework is organized around three role-specific schema macros — pick the one that matches the deployment shape of the crate that's consuming the schema:

* **`include_server_schema!("schema.cstack", db = Postgres)`** — sqlx + axum + procedures + events. Server-side, owns the Postgres database.
* **`include_embedded_schema!("schema.cstack")`** — `cratestack-rusqlite` only. Native mobile/desktop **and** `wasm32-unknown-unknown` (browser, OPFS-backed) from the same source. No sqlx, no axum.
* **`include_client_schema!("schema.cstack")`** — HTTP client stubs only. Treats another service's `.cstack` as a contract; owns no database.

As of `0.4.0` the previous single `cratestack` umbrella crate is split into two strictly disjoint sub-facades that consumers pick between via Cargo's `package =` rename:

```toml
# Backend service (Postgres + Axum + generated Rust client runtime)
cratestack = { package = "cratestack-pg", version = "0.4" }

# Embedded / mobile / desktop / wasm (rusqlite + shared surface)
cratestack = { package = "cratestack-sqlite", version = "0.4" }
```

`cratestack-pg` does not pull in `libsqlite3-sys`, so backend services can depend on the official `sqlx` umbrella crate alongside it without `links = "sqlite3"` collisions. See [`CHANGELOG.md`](./CHANGELOG.md) for the full 0.4.0 migration notes.

What the current slice covers, across those three shapes:

* schema parsing and semantic validation
* compile-time Rust code generation through the three macros above
* SQLx-backed PostgreSQL delegate scaffolding (server)
* embedded SQLite backend via `cratestack-rusqlite`: same `.cstack` schemas, sync API, **same code compiles to native and to `wasm32-unknown-unknown`** via `sqlite-wasm-rs`; no policy enforcement on the client
* generated Axum model and procedure routes (server)
* generated model and procedure policy enforcement (server)
* first-party CBOR and JSON codecs
* generated Rust, Dart, and TypeScript client surfaces
* a standalone `.cstack` language server (`tower-lsp-server` 0.23) and VS Code extension package
* Studio scaffold generation for one or more schemas
* mixin declarations and model `@use(...)` expansion
* **SQL views** (`view <Name> from <Model>, ...`) — read-only, SQL-defined projections over one or more models on both backends; server-side `@@materialized` with `refresh()` via `REFRESH MATERIALIZED VIEW CONCURRENTLY`; same `@@allow("read", …)` policy machinery models use ([ADR-0003](https://cratestack.dev/internals/views-adr))

## Support Matrix

| `.cstack` capability | Status | Notes |
| --- | --- | --- |
| `datasource` | Supported | `provider` accepts `postgresql` (server) or `sqlite` (embedded — native and `wasm32`) |
| `auth` | Supported | Single auth block |
| `mixin` | Supported | Reusable field sets for models |
| `model` | Supported | Includes relation and policy attributes in current slice |
| `type` | Supported | Supports `@custom` fields |
| `enum` | Supported | Enum values are untyped identifiers |
| `procedure` / `mutation procedure` | Supported | Typed args + return type |
| `mcp` | Supported | Parsed as config block |
| `@use(...)` on model | Supported | Expands mixin fields before validation; model-local fields win name conflicts |
| `view` | Supported | Read-only SQL-defined projection over one or more models. `@@server_sql` / `@@embedded_sql` / `@@sql` for the body, `@@materialized` (server-only) for cached views with `refresh()`, `@@no_unique` for views without a natural primary key. `@@allow("read", …)` is enforced on the server backend only — same scope as model policies, which the embedded rusqlite path also doesn't enforce (clients are untrusted; authorization is the server's job). See [ADR-0003](https://cratestack.dev/internals/views-adr). |

## Workspace

The Rust workspace contains these main packages:

* `cratestack-pg`: server-side facade — sqlx (Postgres) + axum + generated Rust client runtime + the shared schema surface. Picked via `cratestack = { package = "cratestack-pg" }`.
* `cratestack-sqlite`: embedded facade — rusqlite (SQLite on native + `wasm32`) + the shared schema surface. Picked via `cratestack = { package = "cratestack-sqlite" }`. Also re-exports `cratestack-client-rust` on native targets so hybrid consumers (NAPI / Tauri shells) can call `include_client_schema!` alongside `include_embedded_schema!`.
* `cratestack-core`: shared metadata, auth context, codec, error, and envelope types
* `cratestack-parser`: `.cstack` parser and semantic checker
* `cratestack-policy`: canonical policy literals, predicates, and procedure-policy evaluation types
* `cratestack-macros`: compile-time schema and client generation
* `cratestack-sql`: dialect-agnostic SQL primitives shared by both backends
* `cratestack-sqlx`: SQLx-backed Postgres runtime and query/delegate primitives
* `cratestack-rusqlite`: embedded SQLite backend (sync, no tokio, no policies; native and `wasm32-unknown-unknown` via `sqlite-wasm-rs`)
* `cratestack-axum`: generated route integration helpers
* `cratestack-client-rust`: generated Rust client runtime
* `cratestack-client-dart`: Dart package generator
* `cratestack-client-typescript`: TypeScript package generator
* `cratestack-client-flutter`: Flutter bridge/runtime experiments
* `cratestack-client-store-sqlite`: SQLite-backed client state store
* `cratestack-client-store-redis`: Redis-backed client state store
* `cratestack-redis`: server-side Redis-backed idempotency and rate-limit stores
* `cratestack-codec-cbor`: CBOR codec
* `cratestack-codec-json`: JSON codec
* `cratestack-cli`: `cratestack` command-line tool
* `cratestack-lsp`: `.cstack` language server
* `cratestack-studio`: admin and testing surface for `.cstack` schemas, served from a `studio.toml`
* `cratestack-studio-generator`: transitional shim that will host `studio eject` in Phase 2 of the studio rewrite

The VS Code extension wrapper lives under `packages/cratestack-vscode`.

## Install Locally

From the repository root:

```sh
cargo build --workspace
cargo run -p cratestack-cli -- --help
```

Build the language server:

```sh
cargo build -p cratestack-lsp
```

Package the VS Code extension:

```sh
cargo build --release -p cratestack-lsp
cd packages/cratestack-vscode
pnpm install
pnpm run package:vsix
```

## Minimal Schema

```cstack
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Principal {
  id String
  role String?
}

model Post {
  id Int @id
  title String
  published Boolean @default(false)
  authorId Int

  author User? @relation(fields:[authorId],references:[id])

  @@allow("read", published == true)
  @@allow("create", auth() != null)
  @@allow("update", auth().role == "admin")
}

model User {
  id Int @id
  email String @unique
  displayName String?

  posts Post[] @relation(fields:[id],references:[authorId])

  @@allow("read", auth() != null)
}

type FeedArgs {
  limit Int?
}

procedure getFeed(args: FeedArgs): Post[]
```

## Mixins

Mixins let you reuse field sets across models without introducing a new runtime type. Declare a
top-level `mixin` block, then apply it inside a model with `@use(...)`.

```cstack
mixin AuditFields {
  createdAt DateTime @default(dbgenerated())
  updatedAt DateTime @default(dbgenerated())
}

model Post {
  @use(AuditFields)

  id Int @id
  title String
}
```

Current mixin rules in this slice:

* mixins are field-only reusable fragments for models
* `@use(...)` expands mixin fields before validation and code generation
* model-local fields win on name conflicts with mixin fields
* mixins must not declare `@id`

Validate a schema:

```sh
cargo run -p cratestack-cli -- check --schema path/to/schema.cstack
cargo run -p cratestack-cli -- check --schema path/to/schema.cstack --format json
```

## Rust Generation

Three macros, one schema. Each emits a `cratestack_schema` module shaped for one deployment role — pick **one per crate** based on what that crate is.

### Server (owns the database)

```rust
use cratestack::include_server_schema;

include_server_schema!("schema.cstack", db = Postgres);
```

Emits sqlx-backed `FromRow<PgRow>` impls, model descriptors, `Cratestack` runtime over `sqlx::PgPool`, generated axum CRUD + procedure routes, host-owned auth wiring, and `events::Subscriptions` for `@@emit`. `db = Postgres` is currently the only accepted value; the parser is wired so future `db = MySql` / `db = Sqlite`-via-sqlx is non-breaking at call sites that already pass `Postgres`.

### Embedded (owns a local SQLite)

```rust
use cratestack::include_embedded_schema;

include_embedded_schema!("schema.cstack");
```

Emits `cratestack-rusqlite`-backed `FromRusqliteRow` impls, model descriptors, and CRUD inputs. No sqlx, no axum, no procedures. Same code compiles for native (mobile via FFI, desktop) **and** for `wasm32-unknown-unknown` (browser via OPFS) — the runtime open path is the only target-specific bit.

### Client (consumes another service)

```rust
use cratestack::include_client_schema;

include_client_schema!("../schemas/billing.cstack");
```

```rust
use cratestack::client_rust::{CborCodec, ClientConfig, CratestackClient};

let base_url = url::Url::parse("https://billing.example.internal")?;
let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
let client = cratestack_schema::client::Client::new(runtime);
```

Emits model + input types, generated typed procedure clients, and a reqwest-backed `Client` facade. No DB, no FromRow impls. The schema is treated purely as a contract.

Generated Rust clients serialize the same HTTP projection contract used by generated routes, including `fields`, `include`, `includeFields[path]`, `sort`, `limit`, `offset`, and grouped `where` expressions.

### Strict split

The three macros are **strictly disjoint** on backend types: `include_server_schema!` never emits rusqlite items, `include_embedded_schema!` never emits sqlx items, `include_client_schema!` never emits either. Each crate pays only for its own surface — no transitive sqlx in mobile builds, no rusqlite in server builds.

## Embedded SQLite (Offline-First, Native + Browser)

The same `.cstack` schema that drives the server can also drive an embedded SQLite database. As of 0.3.0 the embedded backend ships from one source to **three targets**:

* **Native mobile** (iOS, Android via FFI / `flutter_rust_bridge`)
* **Native desktop** (Linux, macOS, Windows)
* **Browser** via `wasm32-unknown-unknown` with **OPFS-backed persistence** (`sqlite-wasm-rs` + `sqlite-wasm-vfs`)

This is the "Rust as real frontend, UI as UI-only" architecture — Rust owns state, persistence, and business logic; the UI layer (Flutter, React, Solid…) talks to Rust over FFI or `wasm-bindgen`.

What's different from the server path:

* **Sync API** — `cratestack-rusqlite` uses `rusqlite` with bundled SQLite, no `tokio`, no async on the data path. Smaller binaries and friendlier FFI/JS bridging.
* **No policy enforcement** — clients are untrusted; authorization is the server's concern. `@@allow` / `@@deny` parse but don't gate reads or writes.
* **Bundled SQLite** — works on every target without a system SQLite to wrangle. On `wasm32-unknown-unknown`, `rusqlite 0.39` swaps its FFI backend to `sqlite-wasm-rs` transparently.

Minimal native usage:

```rust
use cratestack::include_embedded_schema;
use cratestack::{RusqliteRuntime, rusqlite_backend::ddl::create_table_sql};
use cratestack_rusqlite::ModelDelegate;

include_embedded_schema!("schema.cstack");

let runtime = RusqliteRuntime::open("app.db")?;
runtime.with_connection(|conn| {
    conn.execute_batch(&create_table_sql(&cratestack_schema::NOTE_MODEL))?;
    Ok(())
})?;

let notes = ModelDelegate::new(&runtime, &cratestack_schema::NOTE_MODEL);
let created = notes.create(/* CreateNoteInput { ... } */).run()?;
let row = notes.find_unique(created.id).run()?;
```

Minimal browser usage (inside a Dedicated Worker — OPFS `SyncAccessHandle` is worker-only):

```rust
use cratestack::include_embedded_schema;
use cratestack::{RusqliteRuntime, rusqlite_backend};

include_embedded_schema!("schema.cstack");

rusqlite_backend::opfs::install_opfs_vfs(&rusqlite_backend::opfs::OpfsOptions::default()).await?;
let runtime = RusqliteRuntime::open("app.db")?;
```

The wasm32 build needs a wasm-capable clang on `PATH` (`brew install llvm` on macOS; `apt-get install clang lld` on Debian/Ubuntu) — Apple's stock Xcode clang does not include the wasm32 backend. See `crates/cratestack-rusqlite/README.md` for the full build recipe.

## Examples

Runnable, end-to-end examples covering each macro live under [`examples/`](examples) and `crates/cratestack/examples/`. Full index in [`examples/README.md`](examples/README.md).

Pure-Rust (all run under `cargo test --workspace`):

| Use case | Run |
|---|---|
| Smallest embedded program (in-memory DB) | `cargo run --example sqlite_quickstart -p cratestack` |
| Embedded with `Decimal` + filtering | `cargo run --example sqlite_offline_first -p cratestack` |
| JSON FFI envelope dispatcher | `cargo run --example sqlite_ffi_dispatch -p cratestack` |
| Postgres server + axum + procedures | `cargo run --example server_basic -p cratestack` |
| Note-taking CLI on file-backed SQLite | `cargo run -p embedded-cli-example -- --db /tmp/notes.db add "First"` |
| Rust service calling another Rust service | `cargo run -p client-stub-rust-example` |
| BFF / orchestrator (two upstreams) | `cargo run -p client-multi-service-example` |
| Microservice: server + upstream client | `cargo run -p microservice-pair-example` |

Browser (wasm + Vite/Webpack) and mobile (Flutter, Expo) examples land in follow-up PRs.

## Generated HTTP Routes

Generated Axum routes currently support:

* procedure routes
* model CRUD routes
* route-level auth context resolution through host-provided `AuthProvider`
* configured codec handling with CBOR and JSON support
* list-route query parsing for fields, includes, relation include fields, sorting, pagination, scalar filters, grouped `where`, and relation filters
* route-level validation errors for unknown or disallowed query selections
* generated `tracing` instrumentation while subscriber/exporter setup stays host-owned

## Dart Packages

Generate a Flutter-shaped Dart package:

```sh
cargo run -p cratestack-cli -- generate-dart \
  --schema schemas/catalog.cstack \
  --out packages/catalog_client \
  --library-name catalog_client \
  --base-path /api
```

Generated Dart packages expose:

* model and input types
* enum types
* generated selection builders
* generated model and procedure API facades
* a runtime bridge boundary that the host app implements

Regenerate the package after changing the schema or generator templates.

## TypeScript Packages

Generate a TypeScript fetch client plus TanStack Query helpers:

```sh
cargo run -p cratestack-cli -- generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --package-name @example/catalog-client \
  --client-name CatalogClient \
  --base-path /api
```

Generated TypeScript packages include:

* model and input types
* enum types
* a framework-neutral fetch client
* TanStack Query hooks for React and React Native consumers
* projection helpers for generated route query params

## Studio

The studio is an admin and testing surface for `.cstack` schemas. Instead of
a per-project codegen step, you describe the workspace once in a
`studio.toml` and the shipped binary serves the UI:

```sh
cargo run -p cratestack-cli -- studio init     # writes ./studio.toml
cargo run -p cratestack-cli -- studio run      # binds 127.0.0.1:7878
```

A target in `studio.toml` declares one `.cstack`, a `[target.db]` block
(sqlx pool), a `[target.api]` block (deployed cratestack service), or
both. The 0.3.x Jinja-templated `generate-studio` scaffold is gone —
`cratestack studio eject` will replace it in Phase 2 of the rewrite.

## VS Code

CrateStack has two editor surfaces:

* Rust files that consume one of the role-specific schema macros: `cratestack::include_server_schema!`, `cratestack::include_embedded_schema!`, or `cratestack::include_client_schema!`
* `.cstack` schema files

Rust-side editor support is project-dependent because the macros expand relative to a real Cargo project and a real schema path.

Recommended VS Code settings for a consuming project:

```json
{
  "rust-analyzer.linkedProjects": [
    "Cargo.toml"
  ],
  "rust-analyzer.procMacro.enable": true,
  "rust-analyzer.cargo.buildScripts.enable": true,
  "rust-analyzer.checkOnSave": true,
  "rust-analyzer.check.allTargets": true
}
```

For `.cstack` files, use `cratestack-lsp` through `packages/cratestack-vscode` or configure `cratestack.lsp.path` to point at a locally built language server.

## Transport Notes

JSON and CBOR are first-class codecs. COSE is treated as a planned optional envelope layer over encoded bytes.

Generated Axum routes currently enforce a single configured codec per router rather than negotiated multi-codec transport. `application/cbor-seq` is documented as a target transport mode, but it is not implemented yet.

## Current Limits

CrateStack is not yet the right fit for:

* highly customized non-REST transport protocols
* production-stable exact typed non-Rust client generation across arbitrary projection shapes
* full ZenStack-style policy and exposure parity
* runtime custom-field resolution beyond the current generated trait metadata

## Validation

Run the core local checks:

```sh
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
```

Run the VS Code package smoke test:

```sh
cd packages/cratestack-vscode
pnpm install
pnpm run test:smoke
```

## Release

See `RELEASE.md` for the public release process across crates.io, GitHub Releases, VS Code Marketplace, Open VSX, and the docs site.
