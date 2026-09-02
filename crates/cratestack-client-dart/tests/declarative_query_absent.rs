//! cratestack#867 — a `query` block produces **no** Dart client output
//! (accepted design `docs/design/declarative-custom-query.md` §5).
//!
//! Byte-equality against a schema identical except for the `query` block,
//! for both presets — see the TypeScript twin
//! (`cratestack-client-typescript/tests/declarative_query_absent.rs`) for
//! why byte-equality rather than a list of `!contains(...)` probes, and
//! why the shared `type Totals` deliberately stays in both schemas.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const WITHOUT_QUERY: &str = r#"
type Totals {
  total Int
}

model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}
"#;

const WITH_QUERY: &str = r#"
type Totals {
  total Int
}

model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}

query widgetTotals(owner: String): Totals
  @@sql("SELECT COUNT(*)::bigint AS total FROM widgets WHERE owner = $1")
  @allow(auth() != null)
"#;

fn config(preset: DartPreset) -> DartGeneratorConfig {
    DartGeneratorConfig {
        library_name: "widget_client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        preset,
        // Same fingerprint on both sides on purpose — the two schema texts
        // genuinely differ, and that difference is not what is under test.
        schema_sha256: "deadbeef".to_owned(),
        native_cbor: false,
    }
}

fn rendered(source: &str, preset: DartPreset) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    let package = generate_package(&schema, &config(preset)).expect("template should render");
    package
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn the_default_preset_generates_nothing_for_a_query() {
    assert_eq!(
        rendered(WITH_QUERY, DartPreset::Default),
        rendered(WITHOUT_QUERY, DartPreset::Default),
    );
}

#[test]
fn the_riverpod_preset_generates_nothing_for_a_query() {
    assert_eq!(
        rendered(WITH_QUERY, DartPreset::Riverpod),
        rendered(WITHOUT_QUERY, DartPreset::Riverpod),
    );
}

/// Guards the tests above against becoming vacuous — see the TypeScript
/// twin's identical guard.
#[test]
fn the_comparison_is_over_real_generated_output() {
    let files = rendered(WITH_QUERY, DartPreset::Default);
    assert!(files.len() > 1, "expected a real package, got {files:?}");
    assert!(
        files
            .iter()
            .any(|(_, contents)| contents.contains("Widget")),
        "expected the model surface to still be generated",
    );
}
