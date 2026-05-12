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

### `generate-studio` — Studio scaffold

`--schema` and `--service-url` are repeatable and zipped pairwise; pass one of each per service.

```bash
cratestack generate-studio \
  --name catalog-studio \
  --schema schemas/catalog.cstack \
  --service-url https://catalog.example.internal \
  --schema schemas/accounts.cstack \
  --service-url https://accounts.example.internal \
  --out target/catalog-studio
```

Flags:

- `--schema <PATH>` — repeatable, at least one (required)
- `--service-url <URL>` — repeatable, at least one (required)
- `--out <PATH>` (required)
- `--name <NAME>` (required)
- `--context <KEY=VALUE>` — repeatable, propagated to the studio config
- `--mount-path <PATH>` (default `/studio`)
- `--profile <dev|prod>` (default `dev`)
- `--template-dir <PATH>` (optional)

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
