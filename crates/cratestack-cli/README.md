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
  build_runner build --delete-conflicting-outputs` in `--out` so a
  `--preset riverpod` package's `@riverpod` annotations are actually
  expanded (the generated Dart doesn't compile/analyze until
  `build_runner` runs). Off by default. No effect together with `--check`.
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
- `--preset <default|swr>` (default `default`) — `default` is today's
  monolithic layout (`src/models.ts`, `src/client.ts`, ...). `swr` emits
  one `src/models/<model>.ts` per model (types + plain framework-free
  async functions) plus `src/procedures.ts`, the structural foundation
  for SWR hooks. `swr` does not support `transport grpc` schemas yet.

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

### `generate-proto` — `.proto` file + field-number lockfile

```bash
cratestack generate-proto \
  --schema schemas/catalog.cstack \
  --out schemas/catalog.proto \
  --package catalog.v1
```

Emits a `.proto` file describing the schema's messages/enums (no `service`
block — that needs a `transport grpc` schema) plus its sibling field-number
lockfile (`<schema>.pb.lock`) so wire numbers don't silently renumber across
schema edits.

Flags:

- `--schema <PATH>` (required)
- `--out <PATH>` (required)
- `--package <NAME>` — protobuf package name. Required on first run (no
  existing `.pb.lock`); on later runs, must match what's already locked or
  be omitted.
- `--check` (drift-detection mode: rebuild the lock and `.proto` text in
  memory and compare against what's on disk instead of writing)

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
- `cratestack-proto` — `.proto` generator behind `generate-proto`

## License

MIT
