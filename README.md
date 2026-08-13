# CrateStack

CrateStack is a Rust-native, schema-first framework workspace for building typed HTTP APIs, generated clients, and backend services from `.cstack` files.

The implementation is still pre-1.0. As of `0.3.0` the framework is organized around three role-specific schema macros — pick the one that matches the deployment shape of the crate that's consuming the schema:

* **`include_server_schema!("schema.cstack", db = Postgres)`** — sqlx + axum + procedures + events. Server-side, owns the Postgres database.
* **`include_embedded_schema!("schema.cstack")`** — `cratestack-rusqlite` only. Native mobile/desktop **and** `wasm32-unknown-unknown` (browser, OPFS-backed) from the same source. No sqlx, no axum.
* **`include_client_schema!("schema.cstack")`** — HTTP client stubs only. Treats another service's `.cstack` as a contract; owns no database.

As of `0.4.0` the previous single `cratestack` umbrella crate is split into strictly disjoint sub-facades that consumers pick between via Cargo's `package =` rename — a fourth, `cratestack-client`, was added by [cratestack#490](https://github.com/cratestack/cratestack/issues/490):

```toml
# Backend service (Postgres + Axum + generated Rust client runtime)
cratestack = { package = "cratestack-pg", version = "0.7.8" }

# Procedures-only, no-database backend service (Axum + generated Rust
# client runtime, with `sqlx` genuinely absent from the dependency graph)
cratestack = { package = "cratestack-api", version = "0.7.8" }

# Embedded / mobile / desktop / wasm (rusqlite + shared surface)
cratestack = { package = "cratestack-sqlite", version = "0.7.8" }

# Pure HTTP-client SDK (include_client_schema! only, generated Rust client
# runtime, with `cratestack-axum` genuinely absent from the dependency graph)
cratestack = { package = "cratestack-client", version = "0.7.8" }
```

`cratestack-pg` does not pull in `libsqlite3-sys`, so backend services can depend on the official `sqlx` umbrella crate alongside it without `links = "sqlite3"` collisions. `cratestack-client` does not pull in `cratestack-axum` (and therefore `axum`/`tower`/`hyper`/`tower-http`), so a crate that only ever calls a cratestack server doesn't pay for a full server framework it never runs. See [`CHANGELOG.md`](./CHANGELOG.md) for the full 0.4.0 migration notes.

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
* **`datasource { provider = "none" }`** (`db = None`) — a procedures-only server with no database at all: no `model` blocks allowed, and the generated `Cratestack`/router are genuinely `PgPool`-free rather than carrying an always-`None` pool (see [`docs/design/no-database-mode.md`](docs/design/no-database-mode.md))
* **`transport grpc`** — a `.cstack` schema can declare `transport grpc` instead of REST/RPC, generating `.proto` messages (with a field-number lockfile), a tonic service, and Rust/Dart/TypeScript(gRPC-Web) clients; CRUD-only today, procedures and streaming are still follow-up work (see [`docs/design/protobuf.md`](docs/design/protobuf.md))

## Support Matrix

| `.cstack` capability | Status | Notes |
| --- | --- | --- |
| `datasource` | Supported | `provider` accepts `postgresql` (server), `sqlite` (embedded — native and `wasm32`), or `none` (procedures-only server, no database) |
| `datasource { provider = "none" }` (`db = None`) | Supported | Server-only; the schema can never declare a `model`. Generates a genuinely `PgPool`-free `Cratestack`/router, with `ModelRouterState` and the event module omitted entirely rather than compiled in as dead code. See [`docs/design/no-database-mode.md`](docs/design/no-database-mode.md). |
| `transport grpc` | Supported (CRUD only) | Mutually exclusive with REST/RPC transport. Generates `.proto` messages/enums (field-number lockfile), a tonic service, and gRPC clients (Rust, Dart native, TypeScript gRPC-Web). `procedure` declarations aren't wired into the generated gRPC service yet, and there's no streaming support. See [`docs/design/protobuf.md`](docs/design/protobuf.md). |
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
* `cratestack-api`: procedures-only, no-database server facade — Axum + generated Rust client runtime, with `sqlx` genuinely absent from the dependency graph. Picked via `cratestack = { package = "cratestack-api" }` for `datasource { provider = "none" }` schemas.
* `cratestack-sqlite`: embedded facade — rusqlite (SQLite on native + `wasm32`) + the shared schema surface. Picked via `cratestack = { package = "cratestack-sqlite" }`. Also re-exports `cratestack-client-rust` on native targets so hybrid consumers (NAPI / Tauri shells) can call `include_client_schema!` alongside `include_embedded_schema!`.
* `cratestack-client`: pure HTTP-client SDK facade — re-exports only `include_client_schema!` plus the generated Rust client runtime and shared schema surface, with `cratestack-axum` genuinely absent from the dependency graph. Picked via `cratestack = { package = "cratestack-client" }` for a crate that only ever calls a cratestack server.
* `cratestack-core`: shared metadata, auth context, codec, error, and envelope types
* `cratestack-parser`: `.cstack` parser and semantic checker
* `cratestack-policy`: canonical policy literals, predicates, and procedure-policy evaluation types
* `cratestack-macros`: compile-time schema and client generation
* `cratestack-proto`: `.proto` message/enum generator plus the field-number lockfile (`generate-proto`)
* `cratestack-grpc`: server-side tonic service integration for `transport grpc` schemas
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

The VS Code extension wrapper lives under `packages/cratestack-vscode`.

## Install Locally

From the repository root:

```sh
# --exclude embedded_flutter_native: this example crate needs
# flutter_rust_bridge-generated glue that isn't checked in, so a plain
# `--workspace` build fails on a fresh checkout.
cargo build --workspace --exclude embedded_flutter_native
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
`cratestack studio eject --out <dir>` replaces it with a self-contained
starter binary crate (`Cargo.toml`, `studio.toml`, an example schema,
`src/main.rs`). Pass `--with-ui` to also unpack the Leptos+Trunk UI
sources into `<out>/ui/` for front-end customization, and `--name` to
set the project name written into the generated `Cargo.toml`/`README.md`
(defaults to `--out`'s directory basename).

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

### TLS crypto provider (`rustls-no-provider`)

Generated Rust clients (`cratestack-client-rust`'s `CratestackClient`) and Studio's REST `ApiSource` build on `reqwest`'s `rustls-no-provider` feature, not `rustls` — as of #440, this crate no longer forces `aws-lc-rs` as the TLS crypto provider onto every consumer of `cratestack`/`cratestack-pg` (it used to, unconditionally, which broke `*-unknown-linux-musl`/`scratch` container builds and any `cargo-deny` policy banning `aws-lc-rs`, since `aws-lc-rs` needs a C toolchain and `ring` doesn't).

Practically, this means:

* **You don't need to do anything to keep working.** `CratestackClient::new` and `ApiSource::new` install a `ring`-backed `rustls::crypto::CryptoProvider` themselves if the process doesn't already have one — the same zero-config experience as before, just on `ring` instead of `aws-lc-rs`.
* **If your own application installs a provider first** (any backend — `ring`, `aws-lc-rs`, or a custom one — via `rustls::crypto::CryptoProvider::install_default()` before constructing your first `CratestackClient`/`ApiSource`), that choice wins; the fallback above only installs if nothing is set yet.
* **`CratestackClient::with_http_client`** takes a `reqwest::Client` you already built, so this doesn't apply — you're responsible for whatever provider that client needed.

## Current Limits

CrateStack is not yet the right fit for:

* `transport grpc` schemas that declare `procedure`s, or that need server/client streaming — `transport grpc` today covers model CRUD only
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

Run the offline quality pipeline (SAST, secrets, dependency scanning — see [`docs/quality-pipeline.md`](docs/quality-pipeline.md)):

```sh
.ci/quality/run.sh
```

## Release

See `RELEASE.md` for the public release process across crates.io, GitHub Releases, VS Code Marketplace, Open VSX, and the docs site.
