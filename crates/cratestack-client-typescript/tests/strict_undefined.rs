//! Issue #774: `build_environment` runs minijinja under
//! `UndefinedBehavior::Strict`, so a template branching on a field the
//! render context does not provide is an ERROR rather than a silently
//! falsy branch.
//!
//! Why this matters more than it sounds: minijinja's default `Lenient`
//! renders `{% if missing %}` as false. Two shipped defects came from
//! exactly that within one week — `native_cbor` (#765) and
//! `models_import_path` (#764) — each a field on `TemplateContext` and
//! absent from `SwrSchemaContext` while both render the same template.
//! The output compiled and looked right; it just spoke the wrong wire
//! codec, and resolved imports at the wrong directory depth.
//!
//! These tests drive the behaviour through `--template-dir`, which is
//! the only supported way to feed the generator a template this crate
//! does not ship. That is also the surface the Strict flip changes for
//! users, so exercising it here is not a contrivance — it is the
//! blast radius under test.

use std::fs;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

fn blog_schema() -> cratestack_core::Schema {
    cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
        .expect("fixture schema should parse")
}

fn generate_with_template_dir(
    dir: &std::path::Path,
) -> Result<
    cratestack_client_typescript::GeneratedTypeScriptPackage,
    cratestack_client_typescript::TypeScriptGeneratorError,
> {
    generate_package(
        &blog_schema(),
        &TypeScriptGeneratorConfig {
            package_name: "@example/strict-probe".to_owned(),
            template_dir: Some(dir.to_path_buf()),
            schema_sha256: "strictundefinedprobe000000000000000000000000000000000000000".to_owned(),
            ..Default::default()
        },
    )
}

/// Decisive test for the flip. A `--template-dir` override that branches
/// on a field no context provides must FAIL generation.
///
/// Under the old `Lenient` default this same template rendered happily,
/// silently taking the else-branch — which is the exact shape of #765
/// and #764. If this test ever passes-as-ok again, the Strict setting
/// has been lost.
#[test]
fn a_template_branching_on_an_undefined_field_fails_generation() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(
        dir.path().join("README.md.j2"),
        "# probe\n{% if a_field_no_context_provides %}yes{% else %}no{% endif %}\n",
    )
    .expect("override template should write");

    let error = generate_with_template_dir(dir.path())
        .expect_err("an undefined field in `{% if %}` must fail under Strict, not render `no`");

    let rendered = error.to_string();
    assert!(
        rendered.contains("README.md.j2"),
        "the error must name the offending template, got: {rendered}"
    );
    assert!(
        rendered.contains("failed to render"),
        "the failure should surface through the existing TemplateRender path \
         (no new error plumbing was added for this), got: {rendered}"
    );
}

/// The same shape via `{{ }}` interpolation rather than `{% if %}`.
/// Worth its own case because a value substitution failing loudly is
/// what stops a `undefined`/empty string reaching a generated file.
#[test]
fn a_template_interpolating_an_undefined_field_fails_generation() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(
        dir.path().join("README.md.j2"),
        "# probe\n{{ a_field_no_context_provides }}\n",
    )
    .expect("override template should write");

    let error = generate_with_template_dir(dir.path())
        .expect_err("an undefined interpolation must fail under Strict");
    assert!(error.to_string().contains("README.md.j2"), "{error}");
}

/// The control, and the reason this is a behaviour change rather than a
/// breakage: an override that only references fields the context DOES
/// provide keeps working exactly as before. Strict rejects undefined
/// names, not user templates as a category.
#[test]
fn a_template_dir_override_using_real_context_fields_still_renders() {
    let dir = tempfile::tempdir().expect("temp dir");
    fs::write(
        dir.path().join("README.md.j2"),
        "# {{ package_name }}\nbase: {{ base_path }}\n",
    )
    .expect("override template should write");

    let package = generate_with_template_dir(dir.path())
        .expect("an override referencing only real fields must still render under Strict");
    let readme = package
        .files
        .iter()
        .find(|file| file.file_name == "README.md")
        .expect("README.md should be generated");
    assert!(
        readme.contents.contains("# @example/strict-probe"),
        "got: {}",
        readme.contents
    );
}
