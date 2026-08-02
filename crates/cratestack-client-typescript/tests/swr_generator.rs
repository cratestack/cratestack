// Static (CI-safe, no external tooling) coverage for the `swr` preset
// (issue #304): file-set shape, per-model content, the ownership rule's
// shared/inline split, the relation-cycle fixture, and the
// framework-free claim (by text — see `tests/swr_runtime.rs` for the
// actual-Node-execution proof, which is best-effort/skippable since no
// Rust CI job in this repo currently provisions Node).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, TypeScriptPreset, generate_package,
};

#[test]
fn default_preset_output_is_unaffected_by_the_swr_preset_existing() {
    // Belt-and-suspenders alongside the untouched `tests/snapshot.rs`:
    // this crate's default pipeline (`generator.rs::generate_default_package`)
    // is a separate code path from `crate::swr::generate`, so adding the
    // `swr` preset cannot have changed default output. Spot-check a
    // couple of default-only files still exist and `swr`-only files do
    // not leak in.
    let package = generate_for("tiny_rest", TypeScriptPreset::Default);
    assert!(file_named(&package, "src/models.ts").is_some());
    assert!(file_named(&package, "src/client.ts").is_some());
    assert!(file_named(&package, "src/react-query.ts").is_some());
    assert!(file_named(&package, "src/models/shared.ts").is_none());
    assert!(file_named(&package, "src/procedures.ts").is_none());
}

#[test]
fn swr_rest_file_set_matches_the_expected_layout() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "README.md",
            "package.json",
            "src/index.ts",
            "src/models/shared.ts",
            "src/models/widget.ts",
            "src/procedures.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "tsconfig.json",
        ],
        "swr preset's REST file set changed unexpectedly"
    );
}

#[test]
fn swr_rpc_file_set_matches_the_expected_layout() {
    let package = generate_for("tiny_rpc", TypeScriptPreset::Swr);
    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "README.md",
            "package.json",
            "src/cbor-item.ts",
            "src/cbor-seq.ts",
            "src/index.ts",
            "src/links.ts",
            "src/models/shared.ts",
            "src/models/widget.ts",
            "src/procedures.ts",
            "src/runtime.ts",
            "src/stream-terminal.ts",
            "tsconfig.json",
        ],
        "swr preset's RPC file set changed unexpectedly"
    );
}

#[test]
fn swr_per_model_file_has_types_and_plain_functions() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let widget = file(&package, "src/models/widget.ts");

    assert!(widget.contains("export interface Widget {"));
    assert!(widget.contains("export interface CreateWidgetInput {"));
    assert!(widget.contains("export interface UpdateWidgetInput {"));
    assert!(widget.contains("export async function listWidgets("));
    assert!(widget.contains("export async function getWidget("));
    assert!(widget.contains("export async function createWidget("));
    assert!(widget.contains("export async function updateWidget("));
    assert!(widget.contains("export async function deleteWidget("));
    assert!(widget.contains("runtime: CratestackRuntime"));
}

#[test]
fn swr_per_model_functions_are_framework_free() {
    // Static proof half of AC #5 — no React import, no hook. The runtime
    // half (actually calling one against a stub server) is
    // `tests/swr_runtime.rs`.
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    for file in package
        .files
        .iter()
        .filter(|f| f.file_name.ends_with(".ts"))
    {
        assert!(
            !file.contents.contains("\"react\"") && !file.contents.contains("'react'"),
            "{} must not import react:\n{}",
            file.file_name,
            file.contents
        );
        assert!(
            !file.contents.contains("useSWR") && !file.contents.contains("use client"),
            "{} must not reference a hook or client-component directive — issue #305, not #304:\n{}",
            file.file_name,
            file.contents
        );
    }
    let package_json = file(&package, "package.json");
    assert!(
        !package_json.contains("react")
            && !package_json.contains("\"swr\":")
            && !package_json.contains("peerDependencies"),
        "swr preset's package.json must not declare any framework peer dependency yet \
         (no hooks exist until #305):\n{package_json}"
    );
}

#[test]
fn swr_procedures_file_has_args_type_and_plain_function() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let procedures = file(&package, "src/procedures.ts");
    assert!(procedures.contains("export interface EchoNameArgs {"));
    assert!(procedures.contains("export async function echoName("));
    assert!(procedures.contains("runtime: CratestackRuntime"));
}

#[test]
fn swr_index_reexports_every_model_and_procedures() {
    let package = generate_for("tiny_rest", TypeScriptPreset::Swr);
    let index = file(&package, "src/index.ts");
    assert!(index.contains("export * from \"./models/shared\";"));
    assert!(index.contains("export * from \"./models/widget\";"));
    assert!(index.contains("export * from \"./procedures\";"));
}

#[test]
fn swr_preset_rejects_grpc_transport() {
    let schema =
        cratestack_parser::parse_schema_file("../../examples/grpc-widgets/schemas/widgets.cstack")
            .expect("grpc fixture should parse");
    let error = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            preset: TypeScriptPreset::Swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect_err("swr preset must reject transport grpc");
    assert!(matches!(
        error,
        cratestack_client_typescript::TypeScriptGeneratorError::SwrPresetUnsupportedForGrpc
    ));
}

/// Acceptance test for the ownership rule (issue #304's self-review ask:
/// does this actually exercise the shared-vs-owned split, or could it
/// pass by accident on a trivial fixture?). `swr_shared_types.cstack`
/// has: `Status` (enum) used by two models, `Address` (a `type`, whose
/// only entry points are procedures — see the fixture's own header
/// comment and `src/swr/ownership.rs`'s module doc for why) used by two
/// procedures, and `Priority` (enum) used by exactly one model.
#[test]
fn cross_model_type_reuse_places_each_type_in_exactly_one_file() {
    let package = generate_for("swr_shared_types", TypeScriptPreset::Swr);
    let shared = file(&package, "src/models/shared.ts");
    let project = file(&package, "src/models/project.ts");
    let task = file(&package, "src/models/task.ts");
    let procedures = file(&package, "src/procedures.ts");

    // Status: shared, imported by both models, defined nowhere else.
    assert!(shared.contains("export type Status ="));
    assert!(project.contains("import type { Status } from \"./shared\";"));
    assert!(task.contains("import type { Status } from \"./shared\";"));
    assert!(!project.contains("export type Status ="));
    assert!(!task.contains("export type Status ="));

    // Address: shared, imported by procedures.ts, defined nowhere else.
    assert!(shared.contains("export interface Address {"));
    assert!(procedures.contains("import type { Address } from \"./models/shared\";"));
    assert!(!procedures.contains("export interface Address {"));
    assert!(!project.contains("Address"));

    // Priority: owned solely by Task — inline there, absent from shared
    // and from Project.
    assert!(task.contains("export type Priority ="));
    assert!(!shared.contains("Priority"));
    assert!(!project.contains("Priority"));

    // No duplicate top-level type declarations anywhere in the package.
    for name in ["Status", "Address", "Priority"] {
        let total_definitions: usize = package
            .files
            .iter()
            .map(|f| {
                f.contents.matches(&format!("export type {name} =")).count()
                    + f.contents
                        .matches(&format!("export interface {name} {{"))
                        .count()
            })
            .sum();
        assert_eq!(
            total_definitions, 1,
            "{name} must be defined exactly once across the whole package"
        );
    }
}

/// Acceptance test for the relation-cycle fixture (`User` -> `Post[]` ->
/// `User`, AC #9): both model files typecheck-import each other, always
/// as `import type` — never a value import — so there is no runtime
/// import cycle, only a type-only one.
#[test]
fn relation_cycle_uses_type_only_cross_imports_with_no_value_level_cycle() {
    let package = generate_for("swr_relation_cycle", TypeScriptPreset::Swr);
    let user = file(&package, "src/models/user.ts");
    let post = file(&package, "src/models/post.ts");

    assert!(
        user.contains("import type { Post } from \"./post\";"),
        "user.ts must import Post as a type-only import:\n{user}"
    );
    assert!(
        post.contains("import type { User } from \"./user\";"),
        "post.ts must import User as a type-only import:\n{post}"
    );
    // Not a value import of the sibling model anywhere — grep for a
    // bare `import {` (no `type`) naming the other model's symbol.
    assert!(
        !user.contains("import { Post }") && !user.contains("import { Post,"),
        "user.ts must never value-import Post:\n{user}"
    );
    assert!(
        !post.contains("import { User }") && !post.contains("import { User,"),
        "post.ts must never value-import User:\n{post}"
    );
}

fn generate_for(fixture_stem: &str, preset: TypeScriptPreset) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            preset,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("swr preset should render")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    file_named(package, file_name).unwrap_or_else(|| panic!("missing generated file {file_name}"))
}

fn file_named<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> Option<&'a str> {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .map(|file| file.contents.as_str())
}
