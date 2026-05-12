# cratestack-client-dart

Dart package generator for CrateStack services.

## Overview

`cratestack-client-dart` renders a complete Dart package from a parsed `.cstack` schema. It exposes a single `generate_package` entry point used by `cratestack-cli`'s `generate-dart` subcommand; downstream tools can also call it directly.

The generator uses `minijinja` templates. A custom `template_dir` overrides individual templates; missing files fall back to the bundled defaults.

## Installation

This is a build-time crate. End users typically invoke it through the CLI:

```bash
cratestack generate-dart \
  --schema schemas/catalog.cstack \
  --out packages/catalog_client \
  --library-name catalog_client \
  --base-path /api
```

To call the generator from Rust:

```toml
[dependencies]
cratestack-client-dart = "0.2.2"
cratestack-parser = "0.2.2"
```

```rust
use cratestack_client_dart::{DartGeneratorConfig, generate_package};

let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let package = generate_package(&schema, &DartGeneratorConfig {
    library_name: "catalog_client".to_owned(),
    base_path: "/api".to_owned(),
    template_dir: None,
})?;

for file in package.files {
    std::fs::write(out_dir.join(&file.file_name), &file.contents)?;
}
```

## Generated Package Layout

The generator emits files for these template specs:

- `pubspec.yaml`
- `analysis_options.yaml`
- `CHANGELOG.md`
- `README.md`
- `lib/<library_name>.dart` (library entry point)
- `lib/src/constants.dart`
- `lib/src/runtime.dart`
- `lib/src/models.dart`
- `lib/src/queries.dart`
- `lib/src/apis.dart`
- `example/main.dart`
- `test/<library_name>_test.dart`

Generated content covers:

- model and input types
- enum types
- selection / include builders
- model and procedure API facades
- a runtime bridge boundary the host app implements

## See Also

- `cratestack-cli` — `generate-dart` command
- `cratestack-client-flutter` — Flutter bridge runtime
- [Quickstart](https://cratestack.dev/getting-started/quickstart)

## License

MIT
