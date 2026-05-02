# CrateStack

CrateStack is a Rust-native, schema-first framework workspace for building typed HTTP APIs, generated clients, and backend services from `.cstack` files.

The implementation is still pre-1.0. The current slice focuses on:

* schema parsing and semantic validation
* compile-time Rust code generation through `include_schema!`
* client-only Rust code generation through `include_client_macro!`
* SQLx-backed PostgreSQL delegate scaffolding
* generated Axum model and procedure routes
* generated model and procedure policy enforcement
* first-party CBOR and JSON codecs
* generated Rust, Dart, and TypeScript client surfaces
* a standalone `.cstack` language server and VS Code extension package
* Studio scaffold generation for one or more schemas
* mixin declarations and model `@use(...)` expansion

## Support Matrix

| `.cstack` capability | Status | Notes |
| --- | --- | --- |
| `datasource` | Supported | `provider` currently expects `postgresql` |
| `auth` | Supported | Single auth block |
| `mixin` | Supported | Reusable field sets for models |
| `model` | Supported | Includes relation and policy attributes in current slice |
| `type` | Supported | Supports `@custom` fields |
| `enum` | Supported | Enum values are untyped identifiers |
| `procedure` / `mutation procedure` | Supported | Typed args + return type |
| `mcp` | Supported | Parsed as config block |
| `@use(...)` on model | Supported | Expands mixin fields before validation; model-local fields win name conflicts |

## Workspace

The Rust workspace contains these main packages:

* `cratestack`: public facade crate and proc-macro re-exports
* `cratestack-core`: shared metadata, auth context, codec, error, and envelope types
* `cratestack-parser`: `.cstack` parser and semantic checker
* `cratestack-policy`: canonical policy literals, predicates, and procedure-policy evaluation types
* `cratestack-macros`: compile-time schema and client generation
* `cratestack-sqlx`: SQLx runtime and query/delegate primitives
* `cratestack-axum`: generated route integration helpers
* `cratestack-client-rust`: generated Rust client runtime
* `cratestack-client-dart`: Dart package generator
* `cratestack-client-typescript`: TypeScript package generator
* `cratestack-client-flutter`: Flutter bridge/runtime experiments
* `cratestack-client-store-sqlite`: SQLite-backed client state store
* `cratestack-client-store-redis`: Redis-backed client state store
* `cratestack-codec-cbor`: CBOR codec
* `cratestack-codec-json`: JSON codec
* `cratestack-cli`: `cratestack` command-line tool
* `cratestack-lsp`: `.cstack` language server
* `cratestack-studio-generator`: Studio app scaffold generator

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

Validate a schema:

```sh
cargo run -p cratestack-cli -- check --schema path/to/schema.cstack
cargo run -p cratestack-cli -- check --schema path/to/schema.cstack --format json
```

## Rust Generation

Use `include_schema!` in the service that owns the schema and database:

```rust
use cratestack::include_schema;

include_schema!("schema.cstack");
```

Use `include_client_macro!` in callers that only need to consume another service's generated HTTP API:

```rust
use cratestack::include_client_macro;

include_client_macro!("../schemas/billing.cstack");
```

Create a generated Rust client:

```rust
use cratestack::client_rust::{CborCodec, ClientConfig, CratestackClient};

let base_url = url::Url::parse("https://billing.example.internal")?;
let runtime = CratestackClient::new(ClientConfig::new(base_url), CborCodec);
let client = cratestack_schema::client::Client::new(runtime);
```

Generated Rust clients serialize the same HTTP projection contract used by generated routes, including `fields`, `include`, `includeFields[path]`, `sort`, `limit`, `offset`, and grouped `where` expressions.

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

## Studio Generation

Generate a Studio app from one or more schemas:

```sh
cargo run -p cratestack-cli -- generate-studio \
  --schema schemas/catalog.cstack \
  --service-url https://catalog.example.internal \
  --schema schemas/accounts.cstack \
  --service-url https://accounts.example.internal \
  --out target/catalog-studio
```

`generate-studio` currently supports repeated `--schema` and `--service-url` pairs. Manifest-driven Studio generation is not implemented yet.

## VS Code

CrateStack has two editor surfaces:

* Rust files that consume `cratestack::include_schema!(...)`
* `.cstack` schema files

Rust-side editor support is project-dependent because `include_schema!` expands relative to a real Cargo project and a real schema path.

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
