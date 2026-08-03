// Behavioral tests for `--preset riverpod` (issue #301) — file-tree
// shape, the ownership rule exercised end to end through
// `riverpod_shared_ownership.cstack`, the gRPC-unsupported error, and the
// regression guard that the `default` preset stays byte-identical.

use cratestack_client_dart::{
    DartGeneratorConfig, DartGeneratorError, DartPreset, GeneratedDartPackage, generate_package,
};

const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

fn generate(fixture: &str, library_name: &str, preset: DartPreset) -> GeneratedDartPackage {
    let path = format!("tests/fixtures/{fixture}.cstack");
    let schema = cratestack_parser::parse_schema_file(&path)
        .unwrap_or_else(|error| panic!("fixture {path} should parse: {error}"));
    generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: library_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset,
            pb_lock: None,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
        },
    )
    .unwrap_or_else(|error| panic!("{fixture} should generate under {preset:?}: {error}"))
}

fn package_file<'a>(package: &'a GeneratedDartPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("missing generated file {name}\n{:#?}", file_names(package)))
        .contents
        .as_str()
}

fn file_names(package: &GeneratedDartPackage) -> Vec<&str> {
    package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect()
}

/// Regression guard for the whole story: omitting `--preset` (i.e.
/// `DartPreset::default()`) must produce the exact same output as
/// explicitly requesting `DartPreset::Default`, and — since
/// `tests/snapshot.rs`'s snapshots already pin `DartPreset::Default`'s
/// content byte-for-byte and are left unmodified by this story — this
/// also transitively proves omitting `--preset` doesn't drift from the
/// pre-#301 shape.
#[test]
fn omitting_preset_matches_explicit_default_preset() {
    let path = "tests/fixtures/ci_rest.cstack";
    let schema = cratestack_parser::parse_schema_file(path).expect("fixture should parse");

    let config_default_value = DartGeneratorConfig {
        library_name: "ci_rest_client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        preset: DartPreset::default(),
        pb_lock: None,
        schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
    };
    let config_explicit_default = DartGeneratorConfig {
        preset: DartPreset::Default,
        ..config_default_value.clone()
    };

    let a = generate_package(&schema, &config_default_value).expect("should render");
    let b = generate_package(&schema, &config_explicit_default).expect("should render");
    assert_eq!(a, b);
}

#[test]
fn riverpod_preset_is_rejected_for_grpc_transport() {
    let schema =
        cratestack_parser::parse_schema_file("../../examples/grpc-widgets/schemas/widgets.cstack")
            .expect("widgets fixture should parse");

    let error = generate_package(
        &schema,
        &DartGeneratorConfig {
            preset: DartPreset::Riverpod,
            ..DartGeneratorConfig::default()
        },
    )
    .expect_err("riverpod preset should refuse a transport grpc schema");

    assert!(matches!(
        error,
        DartGeneratorError::RiverpodPresetGrpcUnsupported
    ));
}

#[test]
fn riverpod_preset_emits_one_file_per_model_and_a_shared_client_surface() {
    let package = generate(
        "riverpod_shared_ownership",
        "shared_ownership_client",
        DartPreset::Riverpod,
    );

    let expected: Vec<&str> = vec![
        "pubspec.yaml",
        "README.md",
        "CHANGELOG.md",
        "analysis_options.yaml",
        "lib/src/constants.dart",
        "lib/src/runtime.dart",
        "lib/src/queries.dart",
        "lib/src/client.dart",
        "lib/src/procedures.dart",
        "lib/src/models/shared_types.dart",
        "lib/src/models/user.dart",
        "lib/src/models/post.dart",
        "example/main.dart",
        "test/shared_ownership_client_test.dart",
        "lib/shared_ownership_client.dart",
    ];
    let actual = file_names(&package);
    for path in &expected {
        assert!(
            actual.contains(path),
            "expected generated file {path} missing from: {actual:#?}"
        );
    }
    assert_eq!(
        actual.len(),
        expected.len(),
        "unexpected extra/missing files: {actual:#?}"
    );
}

#[test]
fn riverpod_shared_ownership_places_the_enum_in_shared_types_not_in_either_model() {
    let package = generate(
        "riverpod_shared_ownership",
        "shared_ownership_client",
        DartPreset::Riverpod,
    );

    let shared = package_file(&package, "lib/src/models/shared_types.dart");
    let user = package_file(&package, "lib/src/models/user.dart");
    let post = package_file(&package, "lib/src/models/post.dart");

    assert!(
        shared.contains("enum Role {"),
        "Role is referenced by both User and Post, so it must live in shared_types.dart:\n{shared}"
    );
    assert!(
        !user.contains("enum Role {"),
        "user.dart must not duplicate the shared Role enum:\n{user}"
    );
    assert!(
        !post.contains("enum Role {"),
        "post.dart must not duplicate the shared Role enum:\n{post}"
    );
    assert!(
        user.contains("import '../models/shared_types.dart';")
            || user.contains("import 'shared_types.dart';"),
        "user.dart should import shared_types.dart for Role:\n{user}"
    );
}

#[test]
fn riverpod_shared_ownership_inlines_procedure_only_types_into_procedures_dart() {
    let package = generate(
        "riverpod_shared_ownership",
        "shared_ownership_client",
        DartPreset::Riverpod,
    );

    let procedures = package_file(&package, "lib/src/procedures.dart");
    let shared = package_file(&package, "lib/src/models/shared_types.dart");

    assert!(
        procedures.contains("class SearchFilter with SearchFilterMappable {"),
        "SearchFilter is reached only via the search procedure, so it belongs in procedures.dart:\n{procedures}"
    );
    assert!(
        procedures.contains("class Address with AddressMappable {"),
        "Address is reached transitively (search -> SearchFilter -> Address), so it also belongs in procedures.dart:\n{procedures}"
    );
    assert!(
        !shared.contains("class Address with AddressMappable {"),
        "Address is single-locus (Procedures only), so it must not also appear in shared_types.dart:\n{shared}"
    );
}

/// `User` -> `Post[]` / `Post` -> `User` — proves the mutual-import
/// relation cycle actually compiles/resolves (further confirmed by
/// `just verify-dart`'s `flutter analyze` pass over this fixture).
#[test]
fn riverpod_shared_ownership_model_files_mutually_import_across_the_relation_cycle() {
    let package = generate(
        "riverpod_shared_ownership",
        "shared_ownership_client",
        DartPreset::Riverpod,
    );

    let user = package_file(&package, "lib/src/models/user.dart");
    let post = package_file(&package, "lib/src/models/post.dart");

    assert!(
        user.contains("import 'post.dart';"),
        "user.dart should import post.dart for the posts relation:\n{user}"
    );
    assert!(
        post.contains("import 'user.dart';"),
        "post.dart should import user.dart for the author relation:\n{post}"
    );
}

#[test]
fn riverpod_preset_relocates_the_provider_alongside_its_model_api() {
    let package = generate(
        "riverpod_shared_ownership",
        "shared_ownership_client",
        DartPreset::Riverpod,
    );

    let user = package_file(&package, "lib/src/models/user.dart");
    assert!(
        user.contains("class UserApi {"),
        "user.dart should carry UserApi:\n{user}"
    );
    assert!(
        user.contains("Provider<UserApi>((ref) {"),
        "user.dart should carry UserApi's own Provider<UserApi> (relocated, not redesigned):\n{user}"
    );

    let client = package_file(&package, "lib/src/client.dart");
    assert!(
        client.contains("AdapterProvider = Provider<CratestackClientAdapter>((ref) {"),
        "client.dart should keep the package-wide xAdapterProvider:\n{client}"
    );
    assert!(
        client.contains("ClientProvider = Provider<"),
        "client.dart should keep the package-wide xClientProvider:\n{client}"
    );
    assert!(
        !client.contains("class UserApi {"),
        "client.dart must not also carry UserApi — it was relocated, not duplicated:\n{client}"
    );
}

#[test]
fn page_input_procedure_argument_generates_correctly_under_default_preset() {
    let package = generate(
        "page_input_procedure",
        "page_input_client",
        DartPreset::Default,
    );
    let models = package_file(&package, "lib/src/models.dart");

    assert!(models.contains("class PageInput {"));
    assert!(models.contains("class ListFeedArgs {"));
    assert!(models.contains("required this.page,"));
    assert!(models.contains("final PageInput page;"));
}

#[test]
fn page_input_procedure_argument_imports_page_input_under_riverpod_preset() {
    let package = generate(
        "page_input_procedure",
        "page_input_client",
        DartPreset::Riverpod,
    );

    let shared = package_file(&package, "lib/src/models/shared_types.dart");
    assert!(shared.contains("class PageInput {"));

    let procedures = package_file(&package, "lib/src/procedures.dart");
    assert!(
        procedures.contains("import 'models/shared_types.dart';"),
        "procedures.dart should import shared_types.dart for PageInput:\n{procedures}"
    );
    assert!(procedures.contains("page: PageInput"));
}

#[test]
fn find_many_procedure_argument_generates_correctly_under_default_preset() {
    let package = generate(
        "find_many_procedure",
        "find_many_client",
        DartPreset::Default,
    );
    let models = package_file(&package, "lib/src/models.dart");

    // Shared filter-operator primitives (once per package, not per model).
    assert!(models.contains("class StringFilter {"));
    assert!(models.contains("class NumberFilter {"));
    assert!(models.contains("enum SortDirection {"));

    // Per-model `PostWhere`/`PostSortField`/`PostOrderByClause`/`PostFindMany`.
    assert!(models.contains("enum PostSortField {"));
    assert!(models.contains("class PostWhere {"));
    assert!(models.contains("final NumberFilter? id;"));
    assert!(models.contains("final StringFilter? title;"));
    assert!(models.contains("class PostOrderByClause {"));
    assert!(models.contains("final PostSortField field;"));
    assert!(models.contains("final SortDirection direction;"));
    assert!(models.contains("class PostFindMany {"));
    assert!(models.contains("final PostWhere? where;"));
    assert!(models.contains("final List<PostOrderByClause>? orderBy;"));

    assert!(models.contains("class SearchPostsArgs {"));
    assert!(models.contains("required this.query,"));
    assert!(models.contains("final PostFindMany query;"));
    assert!(
        models.contains("query: PostFindMany.fromWire("),
        "SearchPostsArgs.fromWire should decode `query` via PostFindMany, not the old bare `FindMany`:\n{models}"
    );
}

#[test]
fn find_many_procedure_argument_imports_post_find_many_under_riverpod_preset() {
    let package = generate(
        "find_many_procedure",
        "find_many_client",
        DartPreset::Riverpod,
    );

    let shared = package_file(&package, "lib/src/models/shared_types.dart");
    assert!(shared.contains("class StringFilter with StringFilterMappable {"));
    assert!(shared.contains("class NumberFilter with NumberFilterMappable {"));
    assert!(shared.contains("enum SortDirection {"));

    let post = package_file(&package, "lib/src/models/post.dart");
    assert!(post.contains("class PostWhere with PostWhereMappable {"));
    assert!(post.contains("class PostOrderByClause with PostOrderByClauseMappable {"));
    assert!(post.contains("class PostFindMany with PostFindManyMappable {"));

    let procedures = package_file(&package, "lib/src/procedures.dart");
    // `procedures.dart` never spells `SortDirection`/the filter class
    // names directly — only the concrete `PostFindMany` (via
    // `models/post.dart`, which itself imports `shared_types.dart` for
    // its own `PostWhere`/`PostOrderByClause` fields) — so importing
    // `models/shared_types.dart` here too would be a real `unused_import`
    // `flutter analyze` failure (confirmed empirically).
    assert!(
        !procedures.contains("import 'models/shared_types.dart';"),
        "procedures.dart should not import shared_types.dart directly — it never references \
         SortDirection/the filter classes, only PostFindMany:\n{procedures}"
    );
    assert!(
        procedures.contains("import 'models/post.dart';"),
        "procedures.dart should import models/post.dart for PostFindMany:\n{procedures}"
    );
    assert!(procedures.contains("final PostFindMany query;"));
    assert!(procedures.contains("query: PostFindMany.fromWire("));
}
