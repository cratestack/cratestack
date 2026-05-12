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
cratestack-client-typescript = "0.2.2"
cratestack-parser = "0.2.2"
```

```rust
use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let package = generate_package(&schema, &TypeScriptGeneratorConfig {
    package_name: "@example/catalog-client".to_owned(),
    base_path: "/api".to_owned(),
    template_dir: None,
})?;
```

## Generated Package Layout

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
- projection helpers for the generated route query params

## See Also

- `cratestack-cli` — `generate-typescript` command
- `cratestack-client-rust` — Rust client runtime
- [Transport Architecture](https://cratestack.dev/architecture/transport-architecture)

## License

MIT
