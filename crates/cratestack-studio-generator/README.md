# cratestack-studio-generator

Project scaffolding generator for CrateStack services.

## Overview

`cratestack-studio-generator` generates complete project scaffolds from `.cstack` schemas, including Rust project structure, database migrations, CI/CD configuration, and Docker setup.

## Installation

This crate is used by `cratestack-cli` and is typically not used directly.

```toml
[dependencies]
cratestack-studio-generator = "0.2"
```

## Usage

```bash
cratestack generate-studio \
  --schema schema.cstack \
  --out ./my-project \
  --name inventory-studio \
  --service-url http://127.0.0.1:8082
```

## Generated Structure

```
my-project/
├── Cargo.toml
├── schema.cstack
├── src/
│   ├── lib.rs
│   ├── main.rs
│   └── auth.rs
├── migrations/
│   └── 0001_initial.sql
├── .github/
│   └── workflows/
│       └── ci.yml
├── docs/
│   └── README.md
├── Dockerfile
└── docker-compose.yml
```

## Programmatic Use

```rust
use cratestack_studio_generator::generate_studio;
use cratestack_parser::parse_schema_file;

let schema = parse_schema_file("schema.cstack")?;
generate_studio(&schema, "./output")?;
```

## Templates

The generator uses templates for:

- `Cargo.toml.j2` - Rust workspace manifest
- `lib.rs.j2` - Library entry point
- `migrations/*.sql` - Initial database migrations
- `.github/workflows/ci.yml` - CI/CD pipeline
- `Dockerfile.j2` - Container build

## Customization

Provide custom templates:

```rust
use cratestack_studio_generator::StudioConfig;

let config = StudioConfig {
    templates_dir: Some("./custom-templates".into()),
    ..Default::default()
};

generate_studio_with_config(&schema, "./output", config)?;
```

## See Also

- `cratestack-cli` - CLI interface for generation
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT