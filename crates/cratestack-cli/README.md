# cratestack-cli

Command-line tool for `.cstack` schema validation and client/Studio code generation.

## Installation

Prebuilt binaries (macOS x64/arm64, Linux x64/arm64, Windows x64) are attached to every
[GitHub Release](https://github.com/cratestack/cratestack/releases) — no Rust toolchain required.

Via [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall cratestack-cli
```

Via npm (downloads the matching platform binary from GitHub Releases on install):

```bash
npm install --global @cratestack/cli
# or run without installing:
npx @cratestack/cli --help
```

From source, with a Rust toolchain:

```bash
cargo install cratestack-cli
```

Or from the workspace:

```bash
cargo run -p cratestack-cli -- --help
```

## Commands

### `check` — validate a schema

```bash
cratestack check --schema path/to/schema.cstack
cratestack check --schema path/to/schema.cstack --format json
```

Flags:

- `--schema <PATH>` — path to the `.cstack` file (required)
- `--format <human|json>` — output format (default `human`)

On success the human formatter writes `schema OK: <path>`; the JSON formatter prints a `{ ok: true, ... }` document. On error the human formatter renders a diagnostic and exits non-zero; the JSON formatter prints `{ ok: false, diagnostics: [...] }` and exits `1`.

### `generate-dart` — Dart package

```bash
cratestack generate-dart \
  --schema schemas/catalog.cstack \
  --out packages/catalog_client \
  --library-name catalog_client \
  --base-path /api
```

Flags:

- `--schema <PATH>` (required)
- `--out <PATH>` (required)
- `--library-name <NAME>` (default `cratestack_client`)
- `--base-path <PATH>` (default `/api`)
- `--template-dir <PATH>` (optional)
- `--check` (drift-detection mode — see below)
- `--preset <default|riverpod>` (default `default`) — `default` is today's
  monolithic `lib/src/models.dart`/`lib/src/apis.dart` layout. `riverpod`
  emits one file per model under `lib/src/models/`, a shared file for
  cross-model types, procedures in their own file, and package-wide DI
  providers in `lib/src/client.dart`.
- `--run-build-runner` — after generation, shell out to `dart run
  build_runner build --delete-conflicting-outputs` in `--out`. Every preset needs this
  now (issue #668 phase 2/3): every generated data class carries a
  `@CratestackBuilder(...)` annotation that `package:cratestack_builder` expands into a
  `{Class}Builder`; `--preset riverpod` additionally needs the step for its own
  `@riverpod` annotations. The generated Dart doesn't compile/analyze until
  `build_runner` runs. Off by default. No effect together with `--check`.
  Requires a Dart SDK on `PATH`.

### `generate-typescript` (alias `generate-ts`)

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --package-name @example/catalog-client \
  --base-path /api
```

Flags:

- `--schema <PATH>` (required)
- `--out <PATH>` (required)
- `--package-name <NAME>` (default `cratestack-client`)
- `--base-path <PATH>` (default `/api`)
- `--template-dir <PATH>` (optional)
- `--check` (drift-detection mode — see below)
- `--full-selection` (emit fully-required model interfaces, driven by the schema's own nullability, instead of the projection-driven optional-everywhere default — for consumers that never do partial `fields`/`include` selection)
- `--swr` — additionally emit the file-per-model + SWR-hooks layout under
  `src/swr/`: one `src/swr/models/<model>.ts` per model (types + plain
  framework-free async functions) plus a sibling `<model>.hooks.ts` of
  `useSWR`/`useSWRMutation` hooks, and a `src/swr/procedures.ts` (+
  `.hooks.ts`) for procedures — reachable from a consumer as
  `<package-name>/swr` (plus `/swr/models/*`, `/swr/procedures`,
  `/swr/procedures.hooks`) via a `package.json` `exports` subpath.
  Purely additive: the default layout at `src/` is always emitted
  regardless of this flag, `--swr` adds `src/swr/` alongside it rather
  than replacing it (issue #591 — this used to be the mutually-exclusive
  `--preset <default|swr>`; running the generator twice into two
  directories for both layouts is no longer necessary).

  ```bash
  cratestack generate-typescript \
    --schema schemas/catalog.cstack \
    --out packages/catalog-client \
    --swr
  ```
- `--refine` — additionally emit `src/refine.ts`, the
  [`@cratestack/refine`](https://www.npmjs.com/package/@cratestack/refine)
  resource manifest for this schema: one entry per model carrying its `@id`
  field name, `@@paged` flag, and `@version` field, bound to the matching
  generated model API. Purely additive — every other emitted file is
  byte-identical with and without it — and it also adds
  `@cratestack/refine`/`@refinedev/core` to the generated `package.json`'s
  peer/dev dependencies. The emitted manifest is typed `ResourceMap` for
  REST and `RpcResourceMap` for RPC, matching whichever `@cratestack/refine`
  provider that transport ships. Composes freely with `--swr`: the manifest
  binds to the default layout's client class, which is always emitted
  regardless of `--swr`.

  ```bash
  cratestack generate-typescript \
    --schema schemas/catalog.cstack \
    --out packages/catalog-client \
    --refine
  ```
- `--tanstack` — additionally emit `src/react-query.ts`, TanStack Query
  (`useQuery`/`useMutation`) hooks over the default layout's client class,
  re-exported from `src/index.ts`, and add
  `@tanstack/react-query` to the generated `package.json`'s peer/dev
  dependencies. Before this flag existed (issue #617), all three were
  emitted unconditionally, for every schema and every transport. Purely
  additive — every other emitted file is byte-identical with and without
  it. Unlike `--refine`, this composes with EVERY transport: `--tanstack`
  gates the same `src/react-query.ts` that used to be unconditional there
  too, it doesn't add support for a transport that lacked it before.
  Composes freely with `--swr`/`--refine`.

  ```bash
  cratestack generate-typescript \
    --schema schemas/catalog.cstack \
    --out packages/catalog-client \
    --tanstack
  ```
- `--no-native-cbor` — fall back to the pure-TypeScript `jsonRpcCodec`
  instead of the published `@cratestack/cbor` package (napi-rs on Node,
  wasm-bindgen in the browser) as the generated RPC runtime's default
  codec (issue #746). No effect on a REST-transport schema —
  `rest-runtime.ts.j2` has no codec seam at all, so REST output never
  depends on this flag. `@cratestack/cbor-node`'s napi target matrix
  covers `x86_64`/`aarch64` on macOS, glibc Linux and musl Linux (Alpine,
  since cratestack#850) plus `x86_64-pc-windows-msvc` — `win32-arm64` is
  the one remaining gap. There the napi loader fails with a generic
  "Cannot find native binding…" error rather than naming the real cause;
  pass `--no-native-cbor` on that target to fall back to `jsonRpcCodec`,
  which has no native dependency and works everywhere. Purely additive:
  with an RPC-transport schema, every other emitted file is
  byte-identical with and without it; with a REST-transport schema,
  output is byte-identical regardless of this flag.

  ```bash
  cratestack generate-typescript \
    --schema schemas/catalog.cstack \
    --out packages/catalog-client \
    --no-native-cbor
  ```

  **Known bug with `--swr` (issue #765):** on an RPC-transport schema,
  `--swr`'s `src/swr/runtime.ts` ignores this flag entirely and always
  emits `jsonRpcCodec`, while the default layout's `src/runtime.ts` still
  honours it — so `--swr --no-native-cbor` (and even the plain default)
  ships one package with two runtimes speaking different codecs. Not
  intended; REST `--swr` is unaffected since REST has no codec seam.

### `--check` — drift detection (CI guard)

Both `generate-dart` and `generate-typescript` accept `--check`: instead of writing
to `--out`, the command generates in memory and diffs the result file-by-file
against what's already on disk. It exits `0` if they match, and non-zero with a
list of drifted files (modified, missing, or unexpected) otherwise. No files
under `--out` are written or modified in `--check` mode.

```bash
cratestack generate-typescript \
  --schema schemas/catalog.cstack \
  --out packages/catalog-client \
  --package-name @example/catalog-client \
  --base-path /api \
  --check
```

Use this in CI to catch a schema change that nobody regenerated the client for,
or a hand-edit to committed generated code.

### `generate-wiremock` — WireMock stub mappings

```bash
cratestack generate-wiremock \
  --schema schemas/catalog.cstack \
  --out wiremock/mappings \
  --base-path /api
```

Generates WireMock stub mappings straight from the schema's `model`/
`procedure` declarations, so integration/e2e tests can run against a mock
backend whose wire contract can't drift from the real one without
regenerating. `transport rest` model CRUD is stateful (a create is visible
on a later list/get, a delete 404s) but needs more than a plain WireMock —
see `cratestack-mock-wiremock`'s crate docs, its `README.md`, and
`docs/design/wiremock-stubs.md` for what's covered and what running the
stateful stubs costs.

Flags:

- `--schema <PATH>` (required)
- `--out <PATH>` (required)
- `--base-path <PREFIX>` (default `/api`) — prepended to every stub's
  `urlPath`, matching the same-named flag on `generate-dart`/
  `generate-typescript`; must agree with whatever prefix the deployed
  server (and any generated client tested against this mock) use.
- `--check` (drift-detection mode, same semantics as `generate-dart`/
  `generate-typescript` above)

### `studio` — admin and testing surface

Replaces the old `generate-studio` codegen scaffold. The studio reads a
workspace file (`studio.toml`) listing one or more `.cstack` schemas plus
their DB and/or API targets, then serves a single binary.

```bash
cratestack studio init                        # writes ./studio.toml
cratestack studio run                         # binds 127.0.0.1:7878 by default
cratestack studio run --config infra/studio.toml --bind 0.0.0.0:9000
cratestack studio eject --out ./out           # self-contained starter binary crate
cratestack studio eject --out ./out --with-ui # + Leptos/Trunk UI sources under ./out/ui/
```

Subcommand flags:

- `init`: `--out <DIR>` (default `.`), `--force` to overwrite an existing `studio.toml`
- `run`: `--config <PATH>` (default `studio.toml`), `--bind <ADDR>` (default `127.0.0.1:7878`)
- `eject`: `--out <DIR>` (required), `--name <NAME>` (project name written
  into the generated `Cargo.toml`/`README.md`; defaults to `--out`'s
  directory basename), `--force` (overwrite files in a non-empty `--out`),
  `--with-ui` (also unpack the Leptos+Trunk UI sources into `<out>/ui/` for
  front-end customization)

`eject` produces a self-contained Cargo binary crate (`Cargo.toml`,
`README.md`, `studio.toml`, `schemas/example.cstack`, `src/main.rs`) that
embeds the studio against your own schemas.

### `migrate diff` — generate a migration

```bash
cratestack migrate diff \
  --schema schema.cstack \
  --out-dir migrations \
  --backend both \
  --name add_customer_email \
  [--allow-destructive]
```

Diffs the current `.cstack` against the committed snapshot of the
previously-generated schema and writes SQL migrations under
`<out-dir>/<backend>/<timestamp>_<name>/`.

Flags:

- `--schema <PATH>` (required)
- `--out-dir <DIR>` (default `migrations`)
- `--backend <postgres|sqlite|both>` (default `both`)
- `--name <SLUG>` (default `migration`)
- `--allow-destructive` — required to emit a migration containing lossy
  ops (`DropColumn`, `DropTable`, narrowing type changes); without it the
  command refuses to write a destructive migration

See `cratestack-migrate`'s README for the full IR/emitter design.

### `migrate baseline` — adopt an existing database

```bash
cratestack migrate baseline \
  --schema schema.cstack \
  --database-url postgres://user:pass@host:5432/db \
  --out-dir migrations \
  [--backend postgres] \
  [--strict]
```

Points `migrate diff` at a database that already has tables — hand-created,
from a prior tool, or from a previous internal migration system — instead
of the empty schema `migrate diff` otherwise assumes when no snapshot
exists yet. Introspects `--database-url`, diffs the live shape against
`--schema` for a drift report (grouped by table, each change tagged
`safe`/`lossy`/`blocking`), writes the snapshot **from the introspected
shape** (not from `--schema` — see below), and records a synthetic row in
`cratestack_migrations` so the runtime applier (`cratestack-sqlx`) and the
authoring side agree about what's already there.

Drift is reported, not resolved, and does **not** fail the command by
default — matching the adoption use case, where the live database rarely
matches the schema byte-for-byte on day one. Pass `--strict` to flip that:
exit non-zero on any drift, with no snapshot written and no row recorded,
for teams that want baselining to double as a "prove the schema already
matches" CI gate instead of an adoption tool.

Because the snapshot is written from what was actually introspected, a
database with drift bakes that drift into the snapshot as "already true" —
a later `migrate diff` will then propose the DDL to reconcile it, rather
than silently treating undeclared drift as permanent. Refuses to run
(non-zero exit, no writes, no DB round-trip) if a snapshot already exists
at `<out-dir>/postgres/schema.snapshot.json` — baselining an
already-managed backend is almost certainly a mistake.

Flags:

- `--schema <PATH>` (required)
- `--database-url <URL>` (required) — the live Postgres database to
  introspect and to record the baseline row into
- `--out-dir <DIR>` (default `migrations`)
- `--backend <postgres>` (default, and currently the only accepted value —
  baseline is Postgres-only for now)
- `--strict` — fail instead of reporting drift

Postgres-only for v1; no `--backend sqlite`/`both`. See
`docs/design/migrate-baseline.md` for the full design.

### `diff` — schema-change detector

```bash
cratestack diff old.cstack new.cstack
cratestack diff old.cstack new.cstack --json
```

Diffs two `.cstack` schemas and classifies each change by its effect on
the generated wire contract (breaking / additive / internal-only). Exits
non-zero if any breaking change is found, so it can gate CI on schema PRs.

Flags:

- `old` — path to the baseline schema (positional, required)
- `new` — path to the candidate schema (positional, required)
- `--json` — emit machine-readable JSON instead of the human report

### `print-ir` — dump parsed schema IR

```bash
cratestack print-ir --schema schemas/catalog.cstack
```

## Build Integration

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema.cstack");

    let output = std::process::Command::new("cratestack")
        .args(["check", "--schema", "schema.cstack", "--format", "json"])
        .output()
        .expect("failed to run cratestack");

    if !output.status.success() {
        panic!("schema validation failed");
    }
}
```

## See Also

- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- `cratestack-client-dart` — Dart package structure
- `cratestack-client-typescript` — TypeScript package structure
- `cratestack-studio` — Studio server + `eject` scaffold implementation
- `cratestack-migrate` — schema diff / migration generator behind `migrate diff`

## License

MIT
