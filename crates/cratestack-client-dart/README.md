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
  --base-path /api \
  --preset default
```

To call the generator from Rust:

```toml
[dependencies]
cratestack-client-dart = "0.7"
cratestack-parser = "0.7"
```

```rust
use cratestack_client_dart::{DartGeneratorConfig, generate_package};

let schema = cratestack_parser::parse_schema_file("schema.cstack")?;
let package = generate_package(&schema, &DartGeneratorConfig {
    library_name: "catalog_client".to_owned(),
    base_path: "/api".to_owned(),
    template_dir: None,
    ..Default::default()
})?;

for file in package.files {
    std::fs::write(out_dir.join(&file.file_name), &file.contents)?;
}
```

## Generated Package Layout

The generator emits files for these template specs (REST/RPC, `default` preset):

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

## Presets

`--preset` (`DartPreset` in `src/config.rs`) picks the output layout for REST/RPC
schemas — a strict superset of the same content, never a redesign of what's generated:

- `default` — the monolithic `lib/src/models.dart`/`lib/src/apis.dart` layout above.
  Byte-identical output is a hard contract.
- `riverpod` — one file per model under `lib/src/models/<model>.dart` (types generated
  via `dart_mappable`, plus an `XApi` client), a shared file for cross-model types,
  procedures in their own file, and the package-wide DI providers
  (`xAdapterProvider`/`xClientProvider`) in `lib/src/client.dart`. List responses use
  `fast_immutable_collections`' `IList`.

```bash
cratestack generate-dart \
  --schema schemas/catalog.cstack \
  --out packages/catalog_client \
  --library-name catalog_client \
  --preset riverpod \
  --run-build-runner
```

`--run-build-runner` is opt-in: after generation, it shells out to `dart run
build_runner build --delete-conflicting-outputs` in `--out` so a `riverpod` package's
`@riverpod`/`dart_mappable` annotations are actually expanded — the annotated Dart alone
does not compile/analyze until `build_runner` runs. Requires a Dart SDK on `PATH`. Has
no effect together with `--check`.

## See Also

- `cratestack-cli` — `generate-dart` command
- `cratestack-client-flutter` — Flutter bridge runtime
- [Quickstart](https://cratestack.dev/getting-started/quickstart)
- After changing a template in this crate, run `just regen-examples` from the repo root and commit the
  diff — it regenerates the committed `examples/flutter-riverpod/client` example so drift is caught
  locally instead of by CI (cratestack#471).

## License

MIT
