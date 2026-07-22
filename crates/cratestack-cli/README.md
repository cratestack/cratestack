# cratestack-cli

Command-line tool for `.cstack` schema validation and client/Studio code generation.

## Installation

```bash
cargo install cratestack-cli --version 0.2.2
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

### `studio` — admin and testing surface

Replaces the old `generate-studio` codegen scaffold. The studio reads a
workspace file (`studio.toml`) listing one or more `.cstack` schemas plus
their DB and/or API targets, then serves a single binary.

```bash
cratestack studio init               # writes ./studio.toml
cratestack studio run                # binds 127.0.0.1:7878 by default
cratestack studio run --config infra/studio.toml --bind 0.0.0.0:9000
cratestack studio eject --out ./out  # Phase 2 — currently returns NotImplemented
```

Subcommand flags:

- `init`: `--out <DIR>` (default `.`), `--force` to overwrite an existing `studio.toml`
- `run`: `--config <PATH>` (default `studio.toml`), `--bind <ADDR>` (default `127.0.0.1:7878`)
- `eject`: `--config <PATH>` (default `studio.toml`), `--out <DIR>` (required)

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
- `cratestack-studio-generator` — Studio scaffold internals

## License

MIT
