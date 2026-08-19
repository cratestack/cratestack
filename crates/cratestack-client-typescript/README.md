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
  --base-path /api
```

To call the generator from Rust:

```toml
[dependencies]
cratestack-client-typescript = "0.7"
cratestack-parser = "0.7"
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

For the default layout (REST or RPC transport):

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
```

Generated content covers:

- model and input types
- enum types
- a framework-neutral fetch client
- selection / include builders for projection
- projection helpers for the generated route query params — for RPC transport, a typed
  `CratestackRpcListQuery`/`toRpcListInput` builder (the RPC counterpart of REST's
  `CratestackFetchQuery`) in `src/queries.ts`

`src/react-query.ts` (TanStack Query hooks for React and React Native consumers) is NOT
in this default list — see `--tanstack` below (issue #617).

## `--swr` (`TypeScriptGeneratorConfig::swr`)

Additionally emits a file-per-model layout under `src/swr/`, alongside (not instead of)
the default layout above: `src/swr/models/<model>.ts` per model (types plus plain,
framework-free async functions) with a sibling `<model>.hooks.ts` of
`useSWR`/`useSWRMutation` hooks, `src/swr/procedures.ts` (+ `.hooks.ts`) for procedures,
and a `src/swr/swr-keys.ts` shared cache-key factory. Reachable by a consumer as
`<package_name>/swr` (plus `/swr/models/*`, `/swr/procedures`, `/swr/procedures.hooks`)
via a `package.json` `exports` subpath the flag adds.

Purely additive by construction (`tests/swr_generator.rs::swr_rest_file_set_is_additive_to_the_default_layout`
and its RPC counterpart pin this): `swr: false` (the default) leaves every other emitted
file byte-identical to before the flag existed, same discipline as `--refine` below.
Composes freely with `--refine` — both bind to / add onto the default layout, which is
always emitted.

Issue #591 turned this from a mutually-exclusive `--preset <default|swr>` into this
additive flag: a consumer who wanted both layouts used to run the generator twice, into
two directories, and depend on two packages. One caveat worth knowing: the default
layout's `CratestackRuntime` and the `/swr` subtree's `CratestackRuntime` are two
separate classes compiled from the same template (mirroring what "two directories" used
to produce) — structurally identical, but not interchangeable at the type level (private
fields make TypeScript treat them as nominally distinct). Construct the runtime from
whichever entry point you're calling into, and keep that choice consistent within one
module — see the generated package's own `README.md` (rendered by `templates/README.md.j2`)
for the full note.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --swr
```

## `--refine` (`TypeScriptGeneratorConfig::refine`)

Adds one file, `src/refine.ts`, holding the
[`@cratestack/refine`](https://www.npmjs.com/package/@cratestack/refine) resource manifest
for the schema — per model: the generated model API, the `@id` field's name, the
`@@paged` flag, and the `@version` field if there is one. Those four facts are in the
schema but appear nowhere at runtime in the generated client (they live only in its
TypeScript types), so a refine app would otherwise have to restate them by hand. The four
facts are transport-agnostic, so REST and RPC schemas get the same per-resource data —
only the emitted `cratestackRefineResources()`'s return type changes, to whichever
`@cratestack/refine` type matches that transport's provider (`ResourceMap` for REST,
`RpcResourceMap` for RPC).

Additive by construction: with the flag off, every emitted file is byte-identical, and
`tests/refine_generator.rs` asserts that rather than trusting it. With it on, only
`package.json` (peer/dev dependency) and `src/index.ts` (re-export) change alongside the
new file.

Independent of `--swr`: the manifest binds to the default layout's client class, which is
always emitted regardless of that flag.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --refine
```

## `--tanstack` (`TypeScriptGeneratorConfig::tanstack`)

Adds one file, `src/react-query.ts` — TanStack Query (`useQuery`/`useMutation`) hooks
over the default layout's client class — re-exports it from `src/index.ts`, and adds
`@tanstack/react-query` to `package.json`'s peer/dev dependencies.

Before issue #617, all three were emitted unconditionally: every generated client, for
every transport (REST and RPC alike) and regardless of any other flag, carried a
hard `@tanstack/react-query` dependency whether or not the consumer used React. `--tanstack`
finishes the convergence `--swr` (#589) and `--refine` (#571) already went through, where
every framework-specific binding is an additive opt-in and the core typed client stays
framework-free.

Additive by construction: with the flag off, every emitted file is byte-identical except
`package.json` (peer/dev dependency) and `src/index.ts` (re-export) — `tests/snapshot.rs`
pins the flag-off default, and `tests/swr_generator.rs` covers the on/off file-set
difference for REST/RPC. Composes freely with `--swr`/`--refine`.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --tanstack
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
