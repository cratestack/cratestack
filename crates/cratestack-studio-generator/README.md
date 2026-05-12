# cratestack-studio-generator

Studio app scaffold generator for CrateStack services.

## Overview

`cratestack-studio-generator` renders a multi-crate Studio scaffold from one or more parsed `.cstack` schemas. The output is a workspace with three crates — a Leptos-based web front-end, an Axum-based backend with metadata + reverse proxy, and a shared metadata crate — plus a root Cargo workspace, Dockerfile, README, and `.gitignore`.

This crate exposes a single `generate_package` entry point used by `cratestack-cli`'s `generate-studio` subcommand. Downstream tools can call it directly.

## Installation

This is a build-time crate; most users invoke it through the CLI:

```bash
cratestack generate-studio \
  --name catalog-studio \
  --schema schemas/catalog.cstack \
  --service-url https://catalog.example.internal \
  --schema schemas/accounts.cstack \
  --service-url https://accounts.example.internal \
  --out target/catalog-studio
```

To call the generator from Rust:

```toml
[dependencies]
cratestack-studio-generator = "0.2.2"
cratestack-parser = "0.2.2"
```

```rust
use cratestack_studio_generator::{
    StudioGeneratorConfig, StudioGeneratorContext, StudioProfile, generate_package,
};

let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let contexts = vec![StudioGeneratorContext {
    key: "catalog".to_owned(),
    display_name: "Catalog".to_owned(),
    service_name: "catalog".to_owned(),
    schema_path: "schema.cstack".into(),
    service_url: "https://catalog.example.internal".to_owned(),
    schema: &schema,
}];
let config = StudioGeneratorConfig {
    name: "catalog-studio".to_owned(),
    mount_path: "/studio".to_owned(),
    profile: StudioProfile::Dev,
    template_dir: None,
};
let package = generate_package(&contexts, &config)?;
```

`template_dir` lets a caller override individual templates; missing files fall back to the bundled defaults.

## Generated Layout

The scaffold spans four template groups: `root/`, `backend/`, `shared/`, `web/`. Output covers:

- root `Cargo.toml`, `Dockerfile`, `README.md`, `.gitignore`
- `backend/` — Axum service: `main.rs`, `config.rs`, `http.rs`, `metadata.rs`, `proxy.rs`, `static_files.rs`
- `shared/` — `lib.rs`, generated `metadata.json`
- `web/` — Leptos UI with `main.rs`, `app.rs`, `routes.rs`, `state.rs`, `api.rs`, `index.html`, `Trunk.toml`, `package.json`, `tailwind.css`, plus a set of `components/` and `pages/` files (sidebar, table, layout, schema viewer, query/procedures pages, etc.)

The scaffold does **not** generate CI workflows, `docker-compose.yml`, or SQL migrations.

## Profiles

`StudioProfile::Dev` enables dev-only context overrides (e.g. switching `service_url` at runtime). `StudioProfile::Prod` locks the configured URLs into the build.

## See Also

- `cratestack-cli` — `generate-studio` subcommand
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT
