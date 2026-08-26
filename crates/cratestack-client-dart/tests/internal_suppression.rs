//! cratestack#743 — `@@internal("action")` route suppression, Dart
//! client generator coverage (`docs/design/route-suppression.md`).
//! Golden-file style assertions on rendered `apis.dart`/model files:
//! a suppressed verb's method must be ABSENT (not present-but-403),
//! for both the default (REST + RPC) templates and the riverpod
//! preset, and `Create<M>Input` must not be emitted once `create` is
//! suppressed.

use cratestack_client_dart::{
    DartGeneratorConfig, DartPreset, GeneratedDartPackage, generate_package,
};
use cratestack_parser::parse_schema;

const SUPPRESSED_CREATE_REST: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
  @@internal("create")
}
"#;

const UNSUPPRESSED_REST: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}
"#;

fn config(preset: DartPreset) -> DartGeneratorConfig {
    DartGeneratorConfig {
        library_name: "widget_client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        preset,
        schema_sha256: "deadbeef".to_owned(),
        native_cbor: false,
    }
}

fn package_file<'a>(package: &'a GeneratedDartPackage, path: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == path)
        .unwrap_or_else(|| {
            panic!(
                "expected generated file `{path}`; got: {:?}",
                package
                    .files
                    .iter()
                    .map(|f| &f.file_name)
                    .collect::<Vec<_>>()
            )
        })
        .contents
        .as_str()
}

#[test]
fn default_preset_omits_the_suppressed_create_method_and_input_type() {
    let schema = parse_schema(SUPPRESSED_CREATE_REST).expect("fixture schema should parse");
    let package =
        generate_package(&schema, &config(DartPreset::Default)).expect("template should render");
    let apis = package_file(&package, "lib/src/apis.dart");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(
        !apis.contains("Future<Widget> create("),
        "suppressed create() must be absent from apis.dart:\n{apis}"
    );
    assert!(
        !models.contains("class CreateWidgetInput"),
        "suppressed create's input class must be absent from models.dart:\n{models}"
    );
    // Surviving verbs must stay present.
    assert!(apis.contains("Future<List<Widget>> list("));
    assert!(apis.contains("Future<Widget> get("));
    assert!(apis.contains("Future<Widget> update("));
    assert!(apis.contains("Future<Widget> delete("));
    assert!(models.contains("class UpdateWidgetInput"));
}

/// Negative-control sibling, in the same file: with no `@@internal`
/// attribute at all, both the method and the input class are present
/// — proving the absence above is pinned to the attribute.
#[test]
fn default_preset_emits_create_when_nothing_is_suppressed() {
    let schema = parse_schema(UNSUPPRESSED_REST).expect("fixture schema should parse");
    let package =
        generate_package(&schema, &config(DartPreset::Default)).expect("template should render");
    let apis = package_file(&package, "lib/src/apis.dart");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(apis.contains("Future<Widget> create("));
    assert!(models.contains("class CreateWidgetInput"));
}

#[test]
fn riverpod_preset_omits_the_suppressed_create_controller_and_input_type() {
    let schema = parse_schema(SUPPRESSED_CREATE_REST).expect("fixture schema should parse");
    let package = generate_package(&schema, &config(DartPreset::Riverpod))
        .expect("riverpod template should render");
    let widget_file = package_file(&package, "lib/src/models/widget.dart");

    assert!(
        !widget_file.contains("Future<Widget> create("),
        "suppressed create() method must be absent: {widget_file}"
    );
    assert!(
        !widget_file.contains("WidgetCreateController"),
        "suppressed create controller must be absent: {widget_file}"
    );
    assert!(
        !widget_file.contains("class CreateWidgetInput"),
        "suppressed create's input class must be absent: {widget_file}"
    );
    // Surviving verbs must stay present.
    assert!(widget_file.contains("Future<IList<Widget>> list("));
    assert!(widget_file.contains("Future<Widget> get("));
    assert!(widget_file.contains("WidgetUpdateController"));
    assert!(widget_file.contains("WidgetDeleteController"));
}
