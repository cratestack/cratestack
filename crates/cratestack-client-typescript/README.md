# cratestack-client-typescript

TypeScript package generator for CrateStack services.

## Overview

`cratestack-client-typescript` renders a TypeScript package from a parsed `.cstack` schema. It exposes a single `generate_package` entry point used by `cratestack-cli`'s `generate-typescript` subcommand (alias `generate-ts`).

The generator uses `minijinja` templates. A custom `template_dir` overrides individual templates; missing files fall back to the bundled defaults.

## Installation

This is a build-time crate. End users typically invoke it through the CLI:

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --package-name @example/catalog-client \
  --base-path /api \
  --preset default
```

To call the generator from Rust:

```toml
[dependencies]
cratestack-client-typescript = "0.6.7"
cratestack-parser = "0.6.7"
```

```rust
use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let package = generate_package(&schema, &TypeScriptGeneratorConfig {
    package_name: "@example/catalog-client".to_owned(),
    base_path: "/api".to_owned(),
    template_dir: None,
    ..Default::default()
})?;
```

## Generated Package Layout

For the `default` preset (REST or RPC transport):

```
package.json
tsconfig.json
README.md
src/
  index.ts
  runtime.ts
  models.ts
  client.ts
  queries.ts
  react-query.ts
```

Generated content covers:

- model and input types
- enum types
- a framework-neutral fetch client
- selection / include builders for projection
- TanStack Query hooks for React and React Native consumers
- projection helpers for the generated route query params — for RPC transport, a typed
  `CratestackRpcListQuery`/`toRpcListInput` builder (the RPC counterpart of REST's
  `CratestackFetchQuery`) in `src/queries.ts`

For a `transport grpc` schema, the layout instead emits a gRPC-Web client:
`src/runtime.ts` (protobuf encode/decode helpers), `src/client.ts`, `src/react-query.ts`,
and `src/index.ts` — model CRUD only, no `queries.ts` (protobuf fields are typed, not
query-string-shaped) and no procedure surface (procedures aren't wired into the
generated gRPC service). This is selected automatically by the schema's declared
transport, not a CLI flag.

## Presets

`--preset` (`TypeScriptPreset` in `src/config.rs`) picks the output layout for REST/RPC
schemas:

- `default` — today's monolithic layout above (`src/models.ts`, `src/client.ts`, ...),
  byte-identical forever.
- `swr` — a file-per-model layout instead: `src/models/<model>.ts` per model (types plus
  plain framework-free async functions) and `src/procedures.ts` for procedures. This is
  the structural foundation for SWR hooks (not yet emitted by this preset). Not supported
  for `transport grpc` schemas.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --preset swr
```

## `--refine` (`TypeScriptGeneratorConfig::refine`)

Adds one file, `src/refine.ts`, holding the
[`@cratestack/refine`](https://www.npmjs.com/package/@cratestack/refine) `ResourceMap` for
the schema — per model: the generated model API, the `@id` field's name, the `@@paged`
flag, and the `@version` field if there is one. Those four facts are in the schema but
appear nowhere at runtime in the generated client (they live only in its TypeScript
types), so a refine app would otherwise have to restate them by hand.

Additive by construction: with the flag off, every emitted file is byte-identical, and
`tests/refine_generator.rs` asserts that rather than trusting it. With it on, only
`package.json` (peer/dev dependency) and `src/index.ts` (re-export) change alongside the
new file.

REST + `preset = Default` only — `generate_package` returns `RefineRequiresRest` /
`RefineUnsupportedPreset` rather than emitting a file that couldn't type-check. See
`src/refine.rs`'s module doc for why each other combination is structurally impossible
rather than merely unimplemented.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --refine
```

## See Also

- `cratestack-cli` — `generate-typescript` command
- `cratestack-client-rust` — Rust client runtime
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)
- After changing a template in this crate, run `just regen-examples` from the repo root and commit the
  diff — it regenerates the committed `examples/react-vite-swr/client` example so drift is caught
  locally instead of by CI (cratestack#471).

## License

MIT
