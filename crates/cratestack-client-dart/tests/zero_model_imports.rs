//! cratestack#785: what a generated package imports must match what it
//! actually references. A schema with zero `model` blocks used to emit
//! `import 'queries.dart';` into `apis.dart` and `import 'models.dart';`
//! into `queries.dart` regardless — dead lines that `flutter analyze`
//! reports as `unused_import`, which `--fatal-warnings` (Dart's own
//! default, and what `just verify-dart` runs) turns into a failed build
//! for the consumer.
//!
//! Same class as #627, one level down: that was a class *body* rendered
//! for a schema with nothing to put in it, fixed by #629's
//! `{% if procedures | length > 0 %}`; this is an import *line*, which
//! that gate does not reach.
//!
//! Every assertion here comes in a pair — the import is absent, **and**
//! no symbol from the imported file is referenced. A test that only
//! checked for the missing line would still pass if a future change
//! dropped an import that was genuinely needed.
//!
//! `just verify-dart` carries the end-to-end half of this: the
//! `procedures_only_rest` fixture in its default-preset list is
//! generated, built, and run through real `flutter analyze
//! --fatal-warnings`.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};
use cratestack_parser::parse_schema;

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// Symbols `queries.dart` exports that `apis.dart` would name — if none
/// of these appear, the import has nothing to satisfy it.
const QUERIES_SYMBOLS: &[&str] = &[
    "CratestackListQuery",
    "CratestackFetchQuery",
    "CratestackProjection",
    "CratestackSelectionProjection",
];

fn generate(source: &str, preset: DartPreset) -> GeneratedDartPackage {
    let schema = parse_schema(source).expect("fixture schema should parse");
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "zero_model_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("template should render")
}

fn file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
}

fn assert_no_dead_import(contents: &str, import: &str, symbols: &[&str], context: &str) {
    let referenced: Vec<&str> = symbols
        .iter()
        .copied()
        .filter(|symbol| contents.contains(symbol))
        .collect();
    assert!(
        referenced.is_empty(),
        "{context} references {referenced:?}, so `{import}` is NOT dead — this test's premise \
         is wrong, not the template"
    );
    assert!(
        !contents.contains(&format!("import '{import}';")),
        "{context} imports `{import}` while referencing none of its symbols:\n{contents}"
    );
}

const PROCEDURES_ONLY_REST: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

transport rest

procedure ping(message: String): String
"#;

const PROCEDURES_ONLY_RPC: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

transport rpc

procedure ping(message: String): String
"#;

const EMPTY_REST: &str = r#"
datasource db {
  provider = "none"
}

transport rest
"#;

const EMPTY_RPC: &str = r#"
datasource db {
  provider = "none"
}

transport rpc
"#;

/// The exact shape cratestack#785 reports: `transport rest`, one
/// procedure, no models.
#[test]
fn rest_procedures_only_package_has_no_dead_query_imports() {
    let package = generate(PROCEDURES_ONLY_REST, DartPreset::Default);

    assert_no_dead_import(
        file(&package, "lib/src/apis.dart"),
        "queries.dart",
        QUERIES_SYMBOLS,
        "apis.dart",
    );
    assert_no_dead_import(
        file(&package, "lib/src/queries.dart"),
        "models.dart",
        &["PageInfo", "Page<"],
        "queries.dart",
    );
}

/// The half of the report that stays true: `apis.dart`'s own
/// `models.dart` import is still live here, because every procedure gets
/// a generated `{Procedure}Args` wrapper in that file. Without this, the
/// gate above could be satisfied by dropping the import unconditionally.
#[test]
fn rest_procedures_only_package_keeps_the_live_models_import() {
    let apis = file(
        &generate(PROCEDURES_ONLY_REST, DartPreset::Default),
        "lib/src/apis.dart",
    )
    .to_owned();
    assert!(apis.contains("PingArgs"), "apis.dart:\n{apis}");
    assert!(apis.contains("import 'models.dart';"), "apis.dart:\n{apis}");
}

/// A schema with neither models nor procedures leaves `apis.dart`'s
/// `models.dart` import dead too — not in #785's repro (which has one
/// procedure) but reachable, and the same one-line gate covers it.
#[test]
fn empty_schema_package_has_no_dead_imports_on_either_transport() {
    for (source, label) in [(EMPTY_REST, "rest"), (EMPTY_RPC, "rpc")] {
        let package = generate(source, DartPreset::Default);
        let apis = file(&package, "lib/src/apis.dart");
        assert_no_dead_import(
            apis,
            "models.dart",
            &["PingArgs", "Page<", "PageInfo"],
            &format!("{label} apis.dart"),
        );
        if label == "rest" {
            assert_no_dead_import(apis, "queries.dart", QUERIES_SYMBOLS, "rest apis.dart");
        }
    }
}

/// Transport parity (see CLAUDE.md): RPC has no `queries.dart`, but its
/// `apis.dart` carries the same `models.dart` import and the same gate.
/// A procedure-only RPC schema still needs it, for the same `{Procedure}Args`
/// reason as REST.
#[test]
fn rpc_procedures_only_package_keeps_the_live_models_import() {
    let apis = file(
        &generate(PROCEDURES_ONLY_RPC, DartPreset::Default),
        "lib/src/apis.dart",
    )
    .to_owned();
    assert!(apis.contains("PingArgs"), "apis.dart:\n{apis}");
    assert!(apis.contains("import 'models.dart';"), "apis.dart:\n{apis}");
}

/// The control that keeps the gates honest: a schema *with* a model must
/// still emit every import, or the fix would be "delete them all".
#[test]
fn a_schema_with_a_model_keeps_every_import() {
    let package = generate(
        r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

transport rest

model Widget {
  id Int @id
  name String

  @@allow("read", true)
}

procedure ping(message: String): String
"#,
        DartPreset::Default,
    );

    let apis = file(&package, "lib/src/apis.dart");
    assert!(apis.contains("import 'models.dart';"), "apis.dart:\n{apis}");
    assert!(
        apis.contains("import 'queries.dart';"),
        "apis.dart:\n{apis}"
    );

    let queries = file(&package, "lib/src/queries.dart");
    assert!(
        queries.contains("import 'models.dart';"),
        "queries.dart:\n{queries}"
    );
}
