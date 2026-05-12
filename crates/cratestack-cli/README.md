# cratestack-cli

Command-line interface for CrateStack schema validation and code generation.

## Overview

`cratestack-cli` provides commands for validating `.cstack` schemas and generating client code in multiple languages.

## Installation

```bash
cargo install cratestack-cli
```

## Commands

### `check` - Validate Schema

```bash
cratestack check schema.cstack
```

Validate schema for errors:

```bash
cratestack check schema.cstack --json
```

JSON output for programmatic consumption:

```json
{
  "ok": true,
  "schema": "schema.cstack",
  "diagnostics": []
}
```

Error output:

```json
{
  "ok": false,
  "schema": "schema.cstack",
  "diagnostics": [
    {
      "line": 5,
      "start": 23,
      "end": 28,
      "message": "field `email` is missing an @id field"
    }
  ]
}
```

### `generate-dart` - Generate Dart Client

```bash
cratestack generate-dart \
  --schema schema.cstack \
  --out ./dart_client \
  --name my_api_client
```

Options:
- `--schema` - Path to `.cstack` file (required)
- `--out` - Output directory (required)
- `--name` - Package name (required)

### `generate-typescript` - Generate TypeScript Client

```bash
cratestack generate-typescript \
  --schema schema.cstack \
  --out ./ts_client \
  --name my-api-client
```

### `generate-studio` - Scaffold Project

Generate a complete project scaffold:

```bash
cratestack generate-studio \
  --schema schema.cstack \
  --out ./my-project \
  --name inventory-studio \
  --service-url http://127.0.0.1:8082
```

## Build Integration

Use in `build.rs`:

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema.cstack");
    
    let output = std::process::Command::new("cratestack")
        .args(["check", "schema.cstack", "--json"])
        .output()
        .expect("failed to run cratestack");
    
    if !output.status.success() {
        panic!("schema validation failed");
    }
}
```

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Validation error |
| 2 | I/O error |
| 101 | Internal error |

## See Also

- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- `cratestack-client-dart` - Dart client package structure
- `cratestack-client-typescript` - TypeScript client package structure

## License

MIT