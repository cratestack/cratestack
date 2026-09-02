//! cratestack#867 — a `query` block produces **no** TypeScript client
//! output (accepted design `docs/design/declarative-custom-query.md` §5).
//!
//! The assertion is byte-equality of the whole generated package against
//! a schema that is identical except for the `query` block, rather than a
//! list of `!contains(...)` probes. That difference matters: a probe list
//! can only rule out the spellings someone thought to write down, and
//! would quietly stop covering anything a future generator emitted under
//! a name nobody predicted. Byte-equality rules out *every* difference,
//! including files added, removed or reordered.
//!
//! The shared `type Totals { … }` stays in both schemas on purpose. A
//! declared `type` is ordinary client surface whether or not a query
//! returns it, so leaving it in is what keeps this test about the query
//! and not about the type it happens to name — with it removed, the two
//! packages would differ for a reason that has nothing to do with #867.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

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

fn config(swr: bool) -> TypeScriptGeneratorConfig {
    TypeScriptGeneratorConfig {
        package_name: "@example/widget-client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        swr,
        full_selection: false,
        refine: false,
        tanstack: false,
        // Both sides use the same fingerprint on purpose: the real
        // `SCHEMA_SHA256` differs between the two schema texts (they *are*
        // different files), and that difference is not what this test is
        // about.
        schema_sha256: "deadbeef".to_owned(),
        ..Default::default()
    }
}

fn rendered(source: &str, swr: bool) -> Vec<(String, String)> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    let package = generate_package(&schema, &config(swr)).expect("template should render");
    package
        .files
        .into_iter()
        .map(|file| (file.file_name, file.contents))
        .collect()
}

#[test]
fn the_default_preset_generates_nothing_for_a_query() {
    assert_eq!(rendered(WITH_QUERY, false), rendered(WITHOUT_QUERY, false));
}

#[test]
fn the_swr_preset_generates_nothing_for_a_query() {
    assert_eq!(rendered(WITH_QUERY, true), rendered(WITHOUT_QUERY, true));
}

/// Guards the test above against becoming vacuous: if `generate_package`
/// ever started returning an empty file list, byte-equality would still
/// hold and prove nothing. This pins that there is real output being
/// compared.
#[test]
fn the_comparison_is_over_real_generated_output() {
    let files = rendered(WITH_QUERY, false);
    assert!(files.len() > 1, "expected a real package, got {files:?}");
    assert!(
        files
            .iter()
            .any(|(name, contents)| name == "src/client.ts" && contents.contains("Widget")),
        "expected the model surface to still be generated",
    );
}
