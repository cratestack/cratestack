//! What the generated `models.dart` carries for `package:cratestack_builder`
//! to expand into a fluent builder (issue #668 phase 2/3) — the annotation
//! on each data class, and the file-level import/part directive that lets
//! `build_runner` find them.
//!
//! Before this story, `cratestack-client-dart` rendered the `{Class}Builder`
//! classes itself (`templates/model_builder_class.dart.j2`, now deleted) and
//! this file asserted directly on their generated *text* — including a
//! Rust-only exclusion (a to-many relation field on a model class got no
//! `add{Field}` append setter, because Rust's own model builder is built
//! from `scalar_model_fields`, which drops relation fields entirely).
//! `package:cratestack_builder` derives everything about a field purely
//! from the emitted Dart source (`DartType.isDartCoreList` etc — see
//! `dart-packages/cratestack_builder/lib/src/builder_generator.dart`'s own
//! doc), which cannot see "this list is a relation, not a scalar field" —
//! a Dart `List<T>?` constructor parameter is a Dart `List<T>?`
//! constructor parameter either way, so the exclusion is threaded through
//! the annotation instead (`nonDefaultingListFields`) rather than lost.
//! This crate's own coverage is now limited to what it still controls: the
//! `@CratestackBuilder(...)` annotation's arguments (`listDefaults`,
//! `touchFlagFields`, `nonDefaultingListFields`) and the `part` directive —
//! everything downstream of that is `dart-packages/cratestack_builder`'s
//! own contract, covered by its own `test/builder_generator_test.dart`.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// `ci_rpc.cstack` carries both a `ProjectionModel`-kind class with a
/// relation-valued list (`Author.posts`) and `Patch`-kind classes
/// (`UpdateAuthorInput`/`UpdatePostInput`), and is in `just verify-dart`'s
/// fixture list, so the generated output is analyzed by CI too.
fn generated_models() -> String {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/ci_rpc.cstack")
        .expect("fixture schema should parse");
    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "ci_rpc_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("default template should render");

    package
        .files
        .iter()
        .find(|file| file.file_name == "lib/src/models.dart")
        .expect("generated package should contain lib/src/models.dart")
        .contents
        .clone()
}

/// The line(s) immediately preceding `\nclass {class_name} {`, trimmed —
/// asserts against exactly the annotation(s) attached to that class, not
/// against substring matches that could accidentally land inside another
/// class's body.
fn annotation_before<'a>(source: &'a str, class_name: &str) -> &'a str {
    let header = format!("\nclass {class_name} {{");
    let class_start = source
        .find(&header)
        .unwrap_or_else(|| panic!("generated output should declare `class {class_name}`"));
    let before = &source[..class_start];
    let annotation_start = before
        .rfind("\n@CratestackBuilder")
        .unwrap_or_else(|| panic!("`class {class_name}` should be preceded by @CratestackBuilder"));
    before[annotation_start..].trim()
}

#[test]
fn models_dart_imports_the_annotation_package_and_declares_the_builder_part() {
    let models = generated_models();
    assert!(
        models.contains("import 'package:cratestack_annotations/cratestack_annotations.dart';"),
        "generated models.dart should import the annotation package:\n{models}"
    );
    assert!(
        models.contains("part 'models.builder.dart';"),
        "generated models.dart should declare the builder part directive:\n{models}"
    );
}

#[test]
fn no_inline_builder_classes_are_emitted_anymore() {
    let models = generated_models();
    assert!(
        !models.contains("Builder {"),
        "builder emission moved to package:cratestack_builder — models.dart should no \
         longer declare any inline `{{Class}}Builder` class:\n{models}"
    );
}

#[test]
fn projection_model_class_gets_the_default_list_defaults_true_annotation() {
    let models = generated_models();
    // `Author.posts` is a to-many relation-valued list, so it's named in
    // `nonDefaultingListFields` (see this file's module doc) — `Post` has
    // no relation-valued list field, so it stays bare.
    assert_eq!(
        annotation_before(&models, "Author"),
        "@CratestackBuilder(nonDefaultingListFields: {'posts'})"
    );
    assert_eq!(annotation_before(&models, "Post"), "@CratestackBuilder()");
}

#[test]
fn a_type_block_with_a_list_field_also_gets_the_default_annotation() {
    let models = generated_models();
    assert_eq!(
        annotation_before(&models, "PostStatusFilter"),
        "@CratestackBuilder()"
    );
}

#[test]
fn patch_kind_update_input_classes_get_list_defaults_false() {
    let models = generated_models();
    assert_eq!(
        annotation_before(&models, "UpdateAuthorInput"),
        "@CratestackBuilder(listDefaults: false)"
    );
    assert_eq!(
        annotation_before(&models, "UpdatePostInput"),
        "@CratestackBuilder(listDefaults: false)"
    );
}

#[test]
fn create_kind_input_classes_keep_the_default_list_defaults_true() {
    // `Create{Model}Input` is `DataClassKind::Plain`, not `Patch` — an
    // unset list there still defaults to `[]`, same as a projection model.
    let models = generated_models();
    assert_eq!(
        annotation_before(&models, "CreateAuthorInput"),
        "@CratestackBuilder()"
    );
    assert_eq!(
        annotation_before(&models, "CreatePostInput"),
        "@CratestackBuilder()"
    );
}
