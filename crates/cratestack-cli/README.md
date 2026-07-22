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
