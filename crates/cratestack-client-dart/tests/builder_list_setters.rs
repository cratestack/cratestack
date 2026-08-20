//! Which generated Dart classes get an `add{Field}` append setter
//! (cratestack#661), and — more importantly — which deliberately do not.
//!
//! Split from the sibling `generator.rs` rather than appended to it: that
//! file is already well past the repo's ~200-LoC ceiling.
//!
//! The exclusion this pins is easy to regress and impossible to notice from
//! the committed examples, because no example schema declares a list field
//! at all. It was introduced once already: the model class is built from
//! *every* field including relations, so a to-many relation picked up an
//! `addPosts` setter that Rust has no counterpart for — Rust builds model
//! builders from `scalar_model_fields`, which drops relation fields
//! outright. That is precisely the cross-language divergence #661 exists to
//! remove, reintroduced by the fix for it.
//!
//! The inverse case matters just as much: a `type` block's fields go through
//! Rust's `scoped_builder_fields`, which does *not* filter relations, so a
//! list inside a `type` must keep its append setter on both sides. Scoping
//! the exclusion to "any field naming a model" rather than to the model
//! class would break that half.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

/// `ci_rpc.cstack` carries both shapes already — `posts Post[] @relation(..)`
/// on `model Author`, and `statuses PostStatus[]` in `type PostStatusFilter`
/// — and is in `just verify-dart`'s fixture list, so the generated output is
/// analyzed by CI too.
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

/// Extract one `class X { .. }` body, so an assertion about `AuthorBuilder`
/// can't accidentally pass or fail on text belonging to another class.
fn class_body<'a>(source: &'a str, class_name: &str) -> &'a str {
    let header = format!("\nclass {class_name} {{");
    let start = source
        .find(&header)
        .unwrap_or_else(|| panic!("generated output should declare `class {class_name}`"));
    let rest = &source[start + 1..];
    let end = rest
        .find("\n}")
        .unwrap_or_else(|| panic!("`class {class_name}` should be terminated"));
    &rest[..end]
}

#[test]
fn relation_valued_list_on_a_model_gets_no_append_setter() {
    let models = generated_models();
    let body = class_body(&models, "AuthorBuilder");
    assert!(
        !body.contains("addPosts"),
        "`posts` is a relation field; Rust's model builder omits it entirely \
         (scalar_model_fields), so a Dart append setter for it would have no \
         counterpart. Builder body:\n{body}"
    );
}

#[test]
fn scalar_list_in_a_type_block_keeps_its_append_setter() {
    let models = generated_models();
    let body = class_body(&models, "PostStatusFilterBuilder");
    assert!(
        body.contains("addStatuses"),
        "`type` fields go through Rust's scoped_builder_fields, which does not \
         filter relations — this list must keep `addStatuses` on both sides. \
         Builder body:\n{body}"
    );
}

/// The bulk setter is a separate question from the append setter and must
/// survive: Dart's model class genuinely carries relation fields (it is the
/// projection type included relations decode into), so its builder mirrors
/// its own constructor. Only the *append* setter was the #661 divergence.
#[test]
fn relation_valued_list_keeps_its_bulk_setter() {
    let models = generated_models();
    let body = class_body(&models, "AuthorBuilder");
    assert!(
        body.contains("AuthorBuilder posts(List<Post>? value)"),
        "the bulk setter mirrors the Dart class's own field and predates \
         #661; only the append setter is excluded. Builder body:\n{body}"
    );
}
