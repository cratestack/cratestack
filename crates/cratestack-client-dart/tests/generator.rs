use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};
use cratestack_parser::parse_schema;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Deterministic stand-in for `cli_support::hash_schema_source`'s real
/// output (issue #178) — an actual SHA-256 hex digest so assertions
/// exercise the same shape a generated client would really carry, not a
/// contrived string.
const TEST_SCHEMA_SHA256: &str = "13914fdc4b27216d09632c23cec2aa5ea971843166fec36df790de94f2fccccb";

#[test]
fn generates_runtime_based_and_riverpod_client_for_blog_schema() {
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
            .expect("fixture schema should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "blog_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("default template should render");

    let all = package
        .files
        .iter()
        .map(|file| file.contents.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");

    let pubspec = package_file(&package, "pubspec.yaml");
    let readme = package_file(&package, "README.md");
    let changelog = package_file(&package, "CHANGELOG.md");
    let analysis_options = package_file(&package, "analysis_options.yaml");
    let library = package_file(&package, "lib/blog_client.dart");
    let runtime = package_file(&package, "lib/src/runtime.dart");
    let queries = package_file(&package, "lib/src/queries.dart");
    let constants = package_file(&package, "lib/src/constants.dart");
    let models = package_file(&package, "lib/src/models.dart");
    let apis = package_file(&package, "lib/src/apis.dart");
    let example = package_file(&package, "example/main.dart");
    let test_file = package_file(&package, "test/blog_client_test.dart");

    assert_eq!(package.files.len(), 12);
    assert!(pubspec.contains("name: blog_client"));
    assert!(pubspec.contains("flutter:"));
    assert!(pubspec.contains("sdk: flutter"));
    assert!(pubspec.contains("flutter_riverpod: ^3.3.1"));
    assert!(pubspec.contains("cbor: ^6.5.1"));
    assert!(pubspec.contains("dio: ^5.8.0+1"));
    assert!(pubspec.contains("flutter_lints: ^6.0.0"));
    assert!(pubspec.contains("flutter_test:"));
    assert!(readme.contains("# blog_client"));
    assert!(readme.contains("## Adapter Setup"));
    assert!(readme.contains("## Riverpod Setup"));
    assert!(readme.contains("## CRUD Usage"));
    assert!(readme.contains("## Procedure Usage"));
    assert!(readme.contains("## Query Parameters"));
    assert!(readme.contains("## Generated Constants"));
    assert!(readme.contains("## Limitations"));
    assert!(readme.contains("client.procedures.getFeed(GetFeedArgs(...), options: options)"));
    assert!(
        readme.contains("client.procedures.publishPost(PublishPostArgs(...), options: options)")
    );
    assert!(!readme.contains("relationName"));
    assert!(changelog.contains("# 0.1.0"));
    assert!(analysis_options.contains("include: package:flutter_lints/flutter.yaml"));
    assert!(library.contains("export 'src/runtime.dart';"));
    assert!(library.contains("export 'src/apis.dart';"));
    assert!(runtime.contains("import 'package:cbor/simple.dart' as cbor;"));
    assert!(runtime.contains("import 'package:dio/dio.dart';"));
    assert!(runtime.contains("abstract interface class CratestackClientAdapter {"));
    assert!(runtime.contains("class CratestackDioAdapter implements CratestackClientAdapter {"));
    assert!(
        runtime.contains("class CratestackCborDioAdapter implements CratestackClientAdapter {")
    );
    assert!(
        runtime
            .contains("const cratestackUseRustTransportExtraKey = 'cratestackUseRustTransport';")
    );
    assert!(queries.contains("class CratestackFetchQuery {"));
    assert!(queries.contains("abstract interface class CratestackProjection<T> {"));
    assert!(
        queries.contains(
            "class CratestackSelectionProjection<T> implements CratestackProjection<T> {"
        )
    );
    assert!(queries.contains("class CratestackSelectionNode {"));
    assert!(queries.contains(
        "CratestackFetchQuery cratestackSelectionToFetchQuery(CratestackSelectionNode node)"
    ));
    assert!(queries.contains("class PostSelection {"));
    assert!(queries.contains("PostSelection author(["));
    assert!(queries.contains("CratestackListQuery toListQuery({"));
    assert!(queries.contains("CratestackProjection<ProjectedPost> asProjection() {"));
    assert!(queries.contains("class UserIncludeSelection {"));
    assert!(queries.contains("UserIncludeSelection profile(["));
    assert!(queries.contains("/// Scalar fields to keep on the primary resource payload."));
    assert!(queries.contains("/// Declared relation paths to embed in the response."));
    assert!(queries.contains("/// Scalar fields to keep on each included relation payload."));
    assert!(queries.contains("final List<String> fields;"));
    assert!(queries.contains("final List<String> include;"));
    assert!(queries.contains("final Map<String, List<String>> includeFields;"));
    assert!(queries.contains("final String? sort;"));
    assert!(queries.contains("query['fields'] = fields.join(',');"));
    assert!(queries.contains("query['include'] = include.join(',');"));
    assert!(queries.contains("query['sort'] = effectiveSort;"));
    assert!(
        queries
            .contains("throw ArgumentError('sort and orderBy must match when both are provided');")
    );
    assert!(queries.contains("const CratestackFetchQuery({"));
    assert!(apis.contains("import 'package:flutter_riverpod/flutter_riverpod.dart';"));
    assert!(apis.contains("class BlogClientCratestackClient {"));
    assert!(apis.contains("Future<List<Post>> list({"));
    assert!(apis.contains("Future<List<T>> listView<T>({"));
    assert!(apis.contains("Future<Page<Session>> list({"));
    assert!(apis.contains("Future<Page<T>> listView<T>({"));
    assert!(apis.contains("Future<Post> get(int id, {"));
    assert!(apis.contains("Future<T> getView<T>(int id, {"));
    assert!(apis.contains("CratestackFetchQuery? query,"));
    assert!(apis.contains("Future<Post> create(CreatePostInput input, {"));
    assert!(apis.contains("Future<Post> update(int id, UpdatePostInput input, {"));
    assert!(apis.contains("Future<Post> delete(int id, {"));
    assert!(apis.contains("class ProceduresApi {"));
    assert!(apis.contains("Future<List<Post>> getFeed(GetFeedArgs args, {"));
    assert!(apis.contains("Future<Page<Post>> getFeedPage(GetFeedPageArgs args, {"));
    assert!(apis.contains("Future<Post> publishPost(PublishPostArgs args, {"));
    assert!(
        apis.contains(
            "final blogClientAdapterProvider = Provider<CratestackClientAdapter>((ref) {"
        )
    );
    assert!(
        apis.contains(
            "final blogClientClientProvider = Provider<BlogClientCratestackClient>((ref) {"
        )
    );
    assert!(apis.contains("final blogClientUserApiProvider = Provider<UserApi>((ref) {"));
    assert!(
        apis.contains("final blogClientProceduresApiProvider = Provider<ProceduresApi>((ref) {")
    );
    assert!(constants.contains("abstract final class PostFieldNames {"));
    assert!(constants.contains("static const String title = 'title';"));
    assert!(constants.contains("abstract final class PostIncludeNames {"));
    assert!(constants.contains("static const String author = 'author';"));
    assert!(example.contains("import 'package:blog_client/blog_client.dart';"));
    assert!(example.contains("final listQuery = selection.toListQuery("));
    assert!(example.contains("// Generated model API entry points:"));
    assert!(example.contains("// Generated procedures:"));
    assert!(example.contains("// - users"));
    assert!(example.contains("// - getFeed(...)"));
    assert!(test_file.contains("import 'package:blog_client/blog_client.dart';"));
    assert!(test_file.contains("final listQuery = selection.toListQuery("));
    assert!(test_file.contains("where: 'published=true'"));
    assert!(test_file.contains("orFilters: ['published=true', 'published=false']"));
    assert!(test_file.contains("filters: {'status': 'active'}"));
    assert!(test_file.contains("const fetchQuery = CratestackFetchQuery();"));
    assert!(all.contains("package:dio"));
    assert!(!all.contains("CancelToken"));
    assert!(!all.contains("CratestackWireCodec"));
    assert!(models.contains("factory Post.fromWire(CratestackValueMap value) {"));
    assert!(models.contains("class ProjectedPost {"));
    assert!(models.contains("ProjectedUser? get author {"));
    assert!(models.contains("ProjectedProfile? get profile {"));
    assert!(models.contains("CratestackValueMap toWire() {"));
    assert!(models.contains("class UpdatePostInput {"));
    assert!(models.contains("class PageInfo {"));
    assert!(models.contains("class Page<T> {"));
    assert!(models.contains("factory Page.fromWire("));
    assert!(models.contains("final PageInfo pageInfo;"));
    assert!(models.contains("final int? id;"));
    assert!(models.contains("final String? title;"));
    assert!(models.contains("final String? subtitle;"));
    assert!(models.contains("final User? author;"));
    assert!(models.contains("final Profile? profile;"));
    assert!(models.contains("final List<Session>? sessions;"));
    assert!(runtime.contains("Missing required field $ownerName.$fieldName"));

    // Issue #178: constants.dart carries the schema hash the generator
    // was given, and every runtime adapter's header map sends it as
    // `x-cratestack-schema-sha` when present.
    assert!(constants.contains(&format!(
        "const String? cratestackSchemaSha256 = '{TEST_SCHEMA_SHA256}';"
    )));
    assert!(runtime.contains("import 'constants.dart';"));
    assert!(runtime.contains(
        "if (cratestackSchemaSha256 != null)\n            'x-cratestack-schema-sha': cratestackSchemaSha256!,"
    ));
}

#[test]
fn omits_schema_sha_header_wiring_when_config_has_no_hash() {
    // Mirrors the Rust client's `Option<&'static str>` contract: an
    // empty `schema_sha256` (the `Default` value — library-direct usage
    // or tests that don't go through the CLI) must render as `null`,
    // never as an empty-string header value.
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
            .expect("fixture schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");

    let constants = package_file(&package, "lib/src/constants.dart");
    let runtime = package_file(&package, "lib/src/runtime.dart");

    assert!(constants.contains("const String? cratestackSchemaSha256 = null;"));
    assert!(!constants.contains("cratestackSchemaSha256 = '"));
    assert!(runtime.contains("if (cratestackSchemaSha256 != null)"));
}

/// `@custom` was removed in favor of `@computed`
/// (`docs/design/computed-fields.md`) — a computed field on a `type`
/// block is client-wire-visible like any other field (it's never a
/// create/update input on its own, since a computed-bearing `type` is
/// rejected as a procedure argument type at parse time), so it needs no
/// special-casing in the generated Dart class at all.
#[test]
fn preserves_computed_fields_on_generated_types() {
    let schema = cratestack_parser::parse_schema_file(
        "../cratestack-pg/tests/fixtures/computed_fields.cstack",
    )
    .expect("fixture schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(models.contains("class Image {"));
    assert!(models.contains("required this.thumbnailUrl,"));
    assert!(models.contains("final String thumbnailUrl;"));
    assert!(models.contains("'thumbnailUrl': thumbnailUrl,"));
    // The parameterized computed field (`proxyUrl String @computed(params:
    // ProxyParams?)`) is exactly as ordinary a field as `thumbnailUrl` on
    // the wire — its params type only affects the server-side resolver
    // signature and the `computedParams` query parameter on model
    // get/list calls, neither of which a bare `type` class has.
    assert!(models.contains("required this.proxyUrl,"));
    assert!(models.contains("final String proxyUrl;"));
    assert!(models.contains("'proxyUrl': proxyUrl,"));
}

/// A `model` computed field (`docs/design/computed-fields.md`) is part of
/// the response shape but is never a create/update input, filter, or sort
/// key — and a *parameterized* computed field's presence unlocks the
/// optional `computedParams` parameter on `get`/`list`. Bare `@computed`
/// (no params type) does NOT unlock it — the server 422s a `computedParams`
/// key that doesn't name a parameterized field, so a model with only bare
/// computed fields must never be offered the parameter in the first place
/// (see [`bare_computed_field_does_not_unlock_computed_params_on_reads`]
/// for that negative case).
#[test]
fn model_computed_field_is_response_only_and_unlocks_computed_params_on_reads() {
    let schema = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed(params: ProxyParams?)
}
"#,
    )
    .expect("computed-field model schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");

    let models = package_file(&package, "lib/src/models.dart");
    let apis = package_file(&package, "lib/src/apis.dart");

    // Response class: computed field present exactly like any other
    // field (`ProjectionModel` kind forces every field nullable).
    assert!(models.contains("class Image {"), "models.dart:\n{models}");
    assert!(
        models.contains("String? get proxyUrl") || models.contains("final String? proxyUrl;"),
        "Image response class must carry proxyUrl: {models}"
    );

    // Create input: computed field excluded entirely.
    let create_start = models
        .find("class CreateImageInput ")
        .expect("CreateImageInput class should exist");
    let create_end = models[create_start..]
        .find("\nclass CreateImageInputBuilder")
        .map(|offset| create_start + offset)
        .unwrap_or(models.len());
    let create_class = &models[create_start..create_end];
    assert!(
        create_class.contains("storageKey"),
        "CreateImageInput must keep the ordinary field: {create_class}"
    );
    assert!(
        !create_class.contains("proxyUrl"),
        "CreateImageInput must never carry a computed field: {create_class}"
    );

    // Update input: same exclusion.
    let update_start = models
        .find("class UpdateImageInput ")
        .expect("UpdateImageInput class should exist");
    let update_end = models[update_start..]
        .find("\nclass UpdateImageInputBuilder")
        .map(|offset| update_start + offset)
        .unwrap_or(models.len());
    let update_class = &models[update_start..update_end];
    assert!(
        !update_class.contains("proxyUrl"),
        "UpdateImageInput must never carry a computed field: {update_class}"
    );

    // Where/sort: computed field excluded — `ImageWhere` still exists
    // (storageKey is filterable), but never mentions proxyUrl; the sort
    // field enum never carries a proxyUrl variant either.
    assert!(
        models.contains("class ImageWhere "),
        "ImageWhere should still exist for the ordinary filterable field: {models}"
    );
    let where_start = models.find("class ImageWhere ").unwrap();
    let where_end = models[where_start..]
        .find("\nclass ImageWhereBuilder")
        .map(|offset| where_start + offset)
        .unwrap_or(models.len());
    assert!(
        !models[where_start..where_end].contains("proxyUrl"),
        "ImageWhere must never carry a computed field: {}",
        &models[where_start..where_end]
    );
    assert!(
        !models.contains("proxyUrl('proxyUrl')"),
        "ImageSortField must never carry a computed field variant: {models}"
    );

    // `get`/`list` both accept the escape-hatch `computedParams` map and
    // fold it into the request's query parameters via the shared runtime
    // helper — see `rest-runtime.dart.j2`'s `cratestackWithComputedParams`.
    assert!(
        apis.contains("Map<String, Object?>? computedParams,"),
        "ImageApi.get/list must accept computedParams: {apis}"
    );
    assert!(
        apis.contains(
            "queryParameters: cratestackWithComputedParams(query?.toQueryParameters(), computedParams),"
        ),
        "ImageApi.get/list must fold computedParams into the request's query parameters: {apis}"
    );
}

/// Negative counterpart to
/// [`model_computed_field_is_response_only_and_unlocks_computed_params_on_reads`]:
/// a model whose only computed field is bare `@computed` (no params type)
/// must NOT be offered the `computedParams` parameter at all — the server
/// 422s a `computedParams` key naming a field with no params type, so
/// accepting the parameter here would only ever produce a request that
/// fails.
#[test]
fn bare_computed_field_does_not_unlock_computed_params_on_reads() {
    let schema = parse_schema(
        r#"
model Image {
  id Int @id
  storageKey String
  label String @computed
}
"#,
    )
    .expect("bare computed-field model schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");

    let apis = package_file(&package, "lib/src/apis.dart");

    assert!(
        !apis.contains("computedParams"),
        "ImageApi.get/list must not accept computedParams for a model with only bare \
         `@computed` fields: {apis}"
    );
}

/// Regression test for issue #137 — a `type` block field referencing a
/// `model` type. Dart emits every model/type/enum class into one flat
/// `models.dart` file, so there's no module-qualification concern like the
/// Rust macro output has, but this locks the shape in regardless.
#[test]
fn type_block_field_referencing_a_model_generates_correctly() {
    let schema = cratestack_parser::parse_schema_file(
        "../cratestack-pg/tests/fixtures/type_references_model.cstack",
    )
    .expect("fixture schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(models.contains("class SomeModel {"));
    assert!(models.contains("class ApiKeySecret {"));
    assert!(models.contains("required this.model,"));
    assert!(models.contains("final SomeModel model;"));
}

#[test]
fn avoids_procedure_arg_name_collisions_with_schema_types() {
    let schema = parse_schema(
        r#"
type SearchOrdersArgs {
  query String
}

procedure searchOrders(args: SearchOrdersArgs): SearchOrdersArgs
"#,
    )
    .expect("collision schema should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "order_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("order template should render");

    let models = package_file(&package, "lib/src/models.dart");
    let apis = package_file(&package, "lib/src/apis.dart");

    assert!(models.contains("class SearchOrdersArgs {"));
    assert!(models.contains("class SearchOrdersProcedureArgs {"));
    assert!(
        apis.contains("Future<SearchOrdersArgs> searchOrders(SearchOrdersProcedureArgs args, {")
    );
}

#[test]
fn generates_real_dart_enums_for_schema_enum_fields_and_procedures() {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/enums.cstack")
        .expect("enum schema should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "enum_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("enum template should render");

    let models = package_file(&package, "lib/src/models.dart");
    let apis = package_file(&package, "lib/src/apis.dart");

    assert!(models.contains("enum Role {"));
    assert!(models.contains("admin('admin'),"));
    assert!(models.contains("member('member');"));
    assert!(models.contains("const Role(this.wireName);"));
    assert!(models.contains("static Role fromWire(Object? value) {"));
    assert!(models.contains("return Role.admin;"));
    assert!(models.contains("Object toWire() => wireName;"));
    assert!(models.contains("final Role? role;"));
    assert!(models.contains("final Role? maybeRole;"));
    assert!(models.contains("final List<Role>? roles;"));
    assert!(models.contains("value['role'] == null ? null : Role.fromWire(value['role'])"));
    assert!(
        models.contains("value['maybeRole'] == null ? null : Role.fromWire(value['maybeRole'])")
    );
    assert!(models.contains("value['roles'] == null ? null : cratestackAsValueList(value['roles']).map((item) => Role.fromWire(item)).toList(growable: false)"));
    assert!(models.contains("'role': role?.toWire()"));
    assert!(models.contains("'maybeRole': maybeRole?.toWire()"));
    assert!(
        models.contains("'roles': roles?.map((item) => item.toWire()).toList(growable: false)")
    );
    assert!(models.contains("class CreateUserInput {"));
    assert!(models.contains("final Role role;"));
    assert!(models.contains("final List<Role> roles;"));
    assert!(models.contains(
        "Role.fromWire(cratestackRequireWireValue('CreateUserInput', 'role', value['role']))"
    ));
    assert!(models.contains("cratestackAsValueList(cratestackRequireWireValue('CreateUserInput', 'roles', value['roles'])).map((item) => Role.fromWire(item)).toList(growable: false)"));
    assert!(models.contains("'role': role.toWire()"));
    assert!(models.contains("'roles': roles.map((item) => item.toWire()).toList(growable: false)"));
    assert!(models.contains("class ProjectedUser {"));
    assert!(models.contains(
        "Role? get role => _value['role'] == null ? null : Role.fromWire(_value['role']);"
    ));
    assert!(models.contains("Role? get maybeRole => _value['maybeRole'] == null ? null : Role.fromWire(_value['maybeRole']);"));
    assert!(models.contains("List<Role>? get roles => _value['roles'] == null ? null : cratestackAsValueList(_value['roles']).map((item) => Role.fromWire(item)).toList(growable: false);"));
    assert!(models.contains("class RoleFilters {"));
    assert!(models.contains("required this.requiredRole,"));
    assert!(models.contains("final Role requiredRole;"));
    assert!(models.contains("final Role? maybeRole;"));
    assert!(models.contains("required this.roles,"));
    assert!(models.contains("final List<Role> roles;"));
    assert!(!models.contains("final String role;"));
    assert!(!models.contains("final String? role;"));
    assert!(!models.contains("final String? maybeRole;"));
    assert!(apis.contains("Future<Role> resolveRole(ResolveRoleArgs args, {"));
    assert!(apis.contains("Future<List<Role>> listRoles(ListRolesArgs args, {"));
    assert!(apis.contains(
        "return Role.fromWire(cratestackRequireWireValue('Procedure', 'resolveRole', body));"
    ));
    assert!(apis.contains(
        "cratestackAsValueList(cratestackRequireWireValue('Procedure', 'listRoles', body)).map((item) => Role.fromWire(item)).toList(growable: false)"
    ));
}

#[test]
fn prefers_template_override_directory_when_provided() {
    let schema =
        cratestack_parser::parse_schema_file("../cratestack-pg/tests/fixtures/blog.cstack")
            .expect("fixture schema should parse");
    let template_dir = project_tmp_path("template-override");
    if template_dir.exists() {
        fs::remove_dir_all(&template_dir).expect("existing template dir should be removable");
    }
    fs::create_dir_all(&template_dir).expect("template dir should be created");
    // REST schemas resolve their library template through `rest-library.dart.j2`.
    // RPC schemas use `rpc-library.dart.j2`. The blog fixture is REST (default).
    fs::write(
        template_dir.join("rest-library.dart.j2"),
        "// override {{ client_class_name }} {{ model_apis|length }}",
    )
    .expect("override template should write");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "blog_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: Some(template_dir.clone()),
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("override template should render");

    assert_eq!(
        package_file(&package, "lib/blog_client.dart"),
        "// override BlogClientCratestackClient 4"
    );

    fs::remove_dir_all(&template_dir).expect("template dir should be removable");
}

fn project_tmp_path(label: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tmp/client-dart-tests")
        .join(format!("{label}-{suffix}"))
}

#[test]
fn decimal_scalar_maps_to_a_real_decimal_type() {
    // cratestack#498: `Decimal`-typed fields must be a real
    // `package:decimal` value, not the wire-format string they're carried
    // as — see this crate's `dart_types.rs`/`wire_decode.rs`/
    // `wire_encode.rs` doc comments on their "Decimal" arms for why.
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/decimal_scalar.cstack")
        .expect("fixture schema should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "decimal_scalar_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("default template should render");

    let pubspec = package_file(&package, "pubspec.yaml");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(
        pubspec.contains("decimal: ^3.2.6"),
        "pubspec.yaml must declare the `decimal` package dependency, got:\n{pubspec}"
    );
    assert!(
        models.contains("import 'package:decimal/decimal.dart';"),
        "models.dart must import package:decimal, got:\n{models}"
    );
    // The generated `Invoice` model class itself forces every field
    // nullable (`DataClassKind::ProjectionModel` — partial `fields`/
    // `include` projection support, the Dart counterpart to TS's
    // `InterfaceKind::Model`), so even `amountXaf` — required in the
    // schema — is `Decimal?` here; `CreateInvoiceInput` is where the
    // schema's own required-ness survives, asserted below.
    assert!(
        models.contains("final Decimal? amountXaf;"),
        "Invoice.amountXaf must be typed `Decimal?`, got:\n{models}"
    );
    assert!(
        models.contains("final Decimal? discountXaf;"),
        "an optional Decimal field must be typed `Decimal?`, got:\n{models}"
    );
    assert!(
        models.contains(
            "amountXaf: value['amountXaf'] == null ? null : Decimal.parse(value['amountXaf'] as String)"
        ),
        "an optional-in-this-class Decimal field must decode via `Decimal.parse`, got:\n{models}"
    );
    assert!(
        models.contains("'amountXaf': amountXaf?.toString()"),
        "an optional-in-this-class Decimal field must encode via `.toString()`, got:\n{models}"
    );
    assert!(
        models.contains("final Decimal amountXaf;")
            && models.contains(
                "amountXaf: Decimal.parse(cratestackRequireWireValue('CreateInvoiceInput', 'amountXaf', value['amountXaf']) as String)"
            )
            && models.contains("'amountXaf': amountXaf.toString()"),
        "CreateInvoiceInput.amountXaf (schema-required) must be typed `Decimal` \
         and decode/encode via the non-nullable `Decimal.parse`/`.toString()` \
         forms, got:\n{models}"
    );
    assert!(
        models.contains("final Decimal? eq;") && models.contains("class DecimalFilter"),
        "DecimalFilter's comparison operands must be typed `Decimal?`, got:\n{models}"
    );
}

/// Text-level proof (fast — no `flutter analyze` needed for this half; see
/// `tests/fixtures/builder_edge_cases.cstack`'s module doc and
/// `just verify-dart`'s `builder_edge_cases` fixture entry for the real
/// analyzer-backed proof) that the generated fluent-builder template
/// no longer has a static `{Class}.builder()` factory at all (maintainer
/// decision: delete the entry point rather than keep guarding its
/// fallback rename — round 1's `newBuilder` rename was itself unguarded,
/// so a schema with BOTH a `builder` field and a `newBuilder` field
/// reproduced the identical `conflicting_static_and_instance` collision).
/// A required `Json` field's `build()` must still skip the same-type
/// `as Object?` cast that `dart analyze --fatal-warnings` flags as
/// `unnecessary_cast`.
#[test]
fn builder_has_no_static_factory_and_avoids_json_cast_collision() {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/builder_edge_cases.cstack")
        .expect("builder edge-case fixture should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "builder_edge_cases_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("builder edge-case template should render");

    let models = package_file(&package, "lib/src/models.dart");

    // No static factory survives on any data class — neither the
    // un-renamed `builder()` spelling nor round 1's `newBuilder()`
    // fallback. Fields literally named `builder`/`newBuilder` are just
    // ordinary fields/setters now; users construct `{T}Builder()` directly.
    assert!(
        !models.contains("static GadgetBuilder builder() => GadgetBuilder();"),
        "the static factory must be deleted entirely, not merely renamed, got:\n{models}"
    );
    assert!(
        !models.contains("static GadgetBuilder newBuilder() => GadgetBuilder();"),
        "the round-1 `newBuilder` fallback factory must also be gone, got:\n{models}"
    );
    assert!(
        !models.contains(".builder()"),
        "no generated class should retain a static `.builder()` factory call, got:\n{models}"
    );

    // The `builder` and `newBuilder` fields both survive as ordinary
    // fluent setters, proving the defect class (both names colliding with
    // a static factory) is structurally impossible now.
    assert!(
        models.contains("final String builder;") && models.contains("final String newBuilder;"),
        "expected `builder` and `newBuilder` to remain as ordinary Gadget fields, got:\n{models}"
    );
    assert!(
        models.contains("CreateGadgetInputBuilder builder(String value) {")
            && models.contains("CreateGadgetInputBuilder newBuilder(String value) {"),
        "expected both `builder` and `newBuilder` to get ordinary fluent setters on the \
         generated builder, got:\n{models}"
    );

    // A field literally named `build` still gets its `setBuild` setter
    // shim, independent of the (now-removed) `builder` factory.
    assert!(
        models.contains("CreateGadgetInputBuilder setBuild(String value) {"),
        "expected the `build` field's setter to stay renamed to `setBuild`, got:\n{models}"
    );

    // The required `Json` field (`meta`, Dart type `Object?`) must not get
    // a same-type `as Object?` cast in `build()`.
    assert!(
        models.contains(
            "meta: _metaSet ? _meta : (throw StateError('CreateGadgetInput.meta is required but was not set')),"
        ),
        "expected no same-type `as Object?` cast on a required Json field's build(), got:\n{models}"
    );
    assert!(
        !models.contains("_meta as Object?"),
        "a same-type `as Object?` cast on a required Json field trips `unnecessary_cast` under \
         `dart analyze --fatal-warnings`"
    );
}

/// Text-level proof (fast — the real running proof, including `flutter
/// analyze`, lives in `just verify-dart`'s `builder_edge_cases` fixture
/// entry, which now also runs `tests/fixtures/builder_edge_cases_list_test
/// .dart` against the generated package) of issue #661's Dart half:
///
/// 1. `Gadget.tags`/`CreateGadgetInput.tags`/`UpdateGadgetInput.tags`'s
///    generated **constructors** are untouched by the fix — `required
///    this.tags` still appears verbatim on `CreateGadgetInput`, proving
///    `FieldView.required` (shared with the constructor template) was not
///    the flag that changed.
/// 2. The **builder** for a list field no longer tracks a `_tagsSet` flag
///    at all (list fields never throw, so there is nothing to track) and
///    `build()` defaults an unset list to `<String>[]` rather than
///    throwing `StateError`.
/// 3. A fluent `addTags(String)` append setter exists beside the bulk
///    `tags(List<String>)` setter, which still replaces.
/// 4. `UpdateGadgetInput`'s builder gets the same `addTags` setter, and its
///    `build()` still passes the backing field straight through (`tags:
///    _tags,`) — an untouched field stays `null`, not `[]`, preserving the
///    existing "never touched" wire representation every other Patch field
///    already relies on.
#[test]
fn list_field_builder_defaults_to_empty_list_and_gains_an_append_setter() {
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/builder_edge_cases.cstack")
        .expect("builder edge-case fixture should parse");

    let package = generate_package(
        &schema,
        &DartGeneratorConfig {
            library_name: "builder_edge_cases_client".to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            preset: DartPreset::Default,
            schema_sha256: TEST_SCHEMA_SHA256.to_owned(),
            native_cbor: false,
        },
    )
    .expect("builder edge-case template should render");

    let models = package_file(&package, "lib/src/models.dart");

    // (1) Constructors are byte-identical to what a `Required`/`List`-gated
    // `required` flag always produced — the fix must not touch this.
    assert!(
        models.contains("final List<String> tags;") && models.contains("required this.tags,"),
        "CreateGadgetInput's constructor must keep requiring `tags` \
         (unaffected by the builder-only fix), got:\n{models}"
    );
    assert!(
        models.contains("final List<String>? tags;"),
        "Gadget/UpdateGadgetInput's constructor `tags` field must stay \
         nullable/optional, unaffected by the fix, got:\n{models}"
    );

    // (2) No Set-tracking for a list field, and no throw in build().
    assert!(
        !models.contains("_tagsSet"),
        "a list field must never gain `_tagsSet` tracking — it's not \
         builder-required — got:\n{models}"
    );
    assert!(
        !models.contains("CreateGadgetInput.tags is required but was not set"),
        "CreateGadgetInput.tags must never throw for being unset, got:\n{models}"
    );
    assert!(
        models.contains("tags: _tags ?? <String>[],"),
        "CreateGadgetInput.build() must default an unset `tags` to `[]`, \
         got:\n{models}"
    );

    // (3) The append setter exists beside the still-replacing bulk setter.
    assert!(
        models.contains("CreateGadgetInputBuilder tags(List<String> value) {\n    _tags = value;\n    return this;\n  }"),
        "the bulk `tags(...)` setter must still exist and still replace, got:\n{models}"
    );
    assert!(
        models.contains("CreateGadgetInputBuilder addTags(String value) {\n    (_tags = <String>[...?_tags]).add(value);\n    return this;\n  }"),
        "expected a fluent `addTags(String)` append setter that allocates \
         the list on first use, got:\n{models}"
    );

    // (4) Update input: append setter exists too, and an untouched field
    // still passes straight through as `null`, never defaulted to `[]`.
    assert!(
        models.contains("UpdateGadgetInputBuilder addTags(String value) {\n    (_tags = <String>[...?_tags]).add(value);\n    return this;\n  }"),
        "UpdateGadgetInput's builder must also get `addTags`, got:\n{models}"
    );
    assert!(
        models.contains("UpdateGadgetInput build() {\n    return UpdateGadgetInput(\n      builder: _builder,\n      newBuilder: _newBuilder,\n      build: _build,\n      meta: _meta,\n      tags: _tags,\n      note: _note,\n      noteIsSet: _noteSet,\n    );"),
        "an untouched UpdateGadgetInput.tags must stay `null` (the existing \
         untouched-field representation), not default to `[]`, got:\n{models}"
    );

    // (5) `Gadget` itself (the model/`ProjectionModel`-kind builder, not
    // just the `Plain`-kind Create input) also defaults an unset `tags` to
    // `[]` — `Gadget.tags`'s `dart_type` is nullable (`List<String>?`, same
    // as `UpdateGadgetInput`'s) for an unrelated reason — every
    // `ProjectionModel` field is forced nullable to represent "not
    // selected" — so `list_needs_default` must not infer "stay null" from
    // nullability alone the way it used to.
    assert!(
        models.contains("tags: _tags ?? <String>[]"),
        "Gadget's model-class builder must also default an unset `tags` to \
         `[]`, not `null`, got:\n{models}"
    );
}

fn package_file<'a>(
    package: &'a cratestack_client_dart::GeneratedDartPackage,
    name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
        .expect("generated file should exist")
}
