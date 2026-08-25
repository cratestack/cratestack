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

    // `ImageComputedParams.operator==`/`hashCode` are wire-equality —
    // `jsonEncode(toWire())` — which needs `dart:convert` imported. Only
    // gated in when at least one model actually has a computed-params
    // class, so this schema (which does) must carry it.
    assert!(
        models.contains("import 'dart:convert';"),
        "models.dart should import dart:convert for ImageComputedParams's wire-equality: {models}"
    );

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
        .find("\n}")  // end of the class body — `\nclass <X>Builder` no longer
        // follows it, builders moved to package:cratestack_builder (#668 phase 2)
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
        .find("\n}")  // end of the class body — `\nclass <X>Builder` no longer
        // follows it, builders moved to package:cratestack_builder (#668 phase 2)
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
        .find("\n}")  // end of the class body — `\nclass <X>Builder` no longer
        // follows it, builders moved to package:cratestack_builder (#668 phase 2)
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

    // `get`/`list` both accept the typed `ImageComputedParams?` parameter
    // (the typed client computedParams surface — see
    // `docs/design/computed-fields.md`'s "Downstream" section —
    // replaces the v1 untyped `Map<String, Object?>?` escape hatch) and
    // fold its `.toWire()` into the request's query parameters via the
    // shared runtime helper — see `rest-runtime.dart.j2`'s
    // `cratestackWithComputedParams`.
    assert!(
        apis.contains("ImageComputedParams? computedParams,"),
        "ImageApi.get/list must accept a typed ImageComputedParams: {apis}"
    );
    assert!(
        !apis.contains("Map<String, Object?>? computedParams,"),
        "ImageApi.get/list must not fall back to the untyped v1 escape hatch: {apis}"
    );
    assert!(
        apis.contains(
            "queryParameters: cratestackWithComputedParams(query?.toQueryParameters(), computedParams?.toWire()),"
        ),
        "ImageApi.get/list must fold computedParams.toWire() into the request's query parameters: {apis}"
    );

    // The generated `ImageComputedParams` class itself: one optional
    // field per parameterized computed field, typed as the declared
    // params type, plus `toWire()` and hand-rolled value-based
    // `operator ==`/`hashCode` (mandatory — this class doubles as a
    // riverpod family provider argument, see
    // `computed_params_class.dart.j2`'s doc comment).
    let class_start = models
        .find("class ImageComputedParams {")
        .unwrap_or_else(|| panic!("ImageComputedParams class should exist: {models}"));
    let class_end = models[class_start..]
        .find("\nclass ")
        .map(|offset| class_start + offset)
        .unwrap_or(models.len());
    let computed_params_class = &models[class_start..class_end];
    assert!(
        computed_params_class.contains("const ImageComputedParams({"),
        "ImageComputedParams should have a const constructor: {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("this.proxyUrl,"),
        "ImageComputedParams constructor should accept proxyUrl: {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("final ProxyParams? proxyUrl;"),
        "ImageComputedParams.proxyUrl should be typed ProxyParams?: {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("if (proxyUrl != null) 'proxyUrl': proxyUrl!.toWire(),"),
        "ImageComputedParams.toWire() should fold a set proxyUrl through its own toWire(): \
         {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("bool operator ==(Object other)"),
        "ImageComputedParams must carry a value-based operator==: {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("jsonEncode(toWire()) == jsonEncode(other.toWire())"),
        "ImageComputedParams.operator== must compare by wire equality (jsonEncode(toWire())), \
         not field-by-field identity on nested params values: {computed_params_class}"
    );
    assert!(
        computed_params_class.contains("int get hashCode"),
        "ImageComputedParams must carry a matching hashCode: {computed_params_class}"
    );

    // Computed params get the same builder treatment as every other
    // generated data class — which since #668 phase 2 means the same
    // ANNOTATION, with `package:cratestack_builder` expanding it at the
    // consumer's build_runner step. Asserting the annotation rather than
    // `class ImageComputedParamsBuilder {` is what keeps this test honest:
    // the inline class it used to look for is exactly what phase 2 removed,
    // and #729's builder would have been silently lost if the annotation
    // had not been added in its place.
    assert!(
        models.contains("@CratestackBuilder()\nclass ImageComputedParams {"),
        "ImageComputedParams must carry the builder annotation like every other \
         generated data class: {models}"
    );
    // The setters and `build()` themselves are package:cratestack_builder's
    // contract now, derived from the constructor it can see — proven by that
    // package's own tests and end-to-end by `computed_params_wire_equality.rs`,
    // which runs build_runner and a real `flutter test` over the output. What
    // this crate still owns, and what is asserted above, is that the class is
    // annotated and the part directive exists. Re-asserting the setter text
    // here would just re-test the package through a second copy of its rules.
    assert!(
        models.contains("part 'models.builder.dart';"),
        "models.dart must declare the part build_runner writes: {models}"
    );
}

/// Two *distinct* parameterized `@computed` fields on the same model — the
/// N>1 case the single-field fixture above can't exercise. Both fields must
/// get their own `ImageComputedParamsBuilder` setter, typed with their own
/// declared params type.
#[test]
fn computed_params_builder_gets_a_setter_per_parameterized_field() {
    let schema = parse_schema(
        r#"
type ProxyParams {
  width Int?
}

type CaptionParams {
  locale String?
}

model Image {
  id Int @id
  storageKey String
  proxyUrl String @computed(params: ProxyParams?)
  captionUrl String @computed(params: CaptionParams?)
}
"#,
    )
    .expect("two-parameterized-field model schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");

    let models = package_file(&package, "lib/src/models.dart");

    // Since #668 phase 2 the builder itself is emitted by
    // `package:cratestack_builder` from the annotation below, not inline
    // here — so this asserts the contract this crate still owns: that the
    // class is annotated at all, and that the `part` directive exists for
    // build_runner to expand into. The setters themselves (one per
    // parameterized field, typed from the constructor) are the package's
    // contract and are covered by
    // `dart-packages/cratestack_builder/test/builder_generator_test.dart`,
    // plus end-to-end by `computed_params_wire_equality.rs`, which runs
    // build_runner and then a real `flutter test` over the result.
    assert!(
        models.contains("@CratestackBuilder()\nclass ImageComputedParams {"),
        "ImageComputedParams must carry the builder annotation: {models}"
    );
    assert!(
        models.contains("part 'models.builder.dart';"),
        "models.dart must declare the part build_runner writes: {models}"
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
    let models = package_file(&package, "lib/src/models.dart");

    assert!(
        !apis.contains("computedParams"),
        "ImageApi.get/list must not accept computedParams for a model with only bare \
         `@computed` fields: {apis}"
    );
    assert!(
        !models.contains("import 'dart:convert';"),
        "models.dart must not import dart:convert when no model has a computed-params class \
         (an unused import fails `flutter analyze --fatal-warnings`): {models}"
    );
}

/// RPC-transport counterpart to
/// [`model_computed_field_is_response_only_and_unlocks_computed_params_on_reads`]
/// — the typed client computedParams surface (see
/// `docs/design/computed-fields.md`'s "Downstream" section) closes the
/// v1 gap where RPC mode had no `computedParams` surface at
/// all. `get`/`list` both accept the gated typed
/// `ImageComputedParams?` parameter and fold its `.toWire()` output
/// into the RPC input frame's `computedParams` key as JSON text via the
/// shared `cratestackWithRpcComputedParams` runtime helper.
#[test]
fn rpc_model_computed_field_unlocks_typed_computed_params_on_get_and_list() {
    let schema = parse_schema(
        r#"
transport rpc

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
    .expect("rpc computed-field model schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");

    let apis = package_file(&package, "lib/src/apis.dart");
    let runtime = package_file(&package, "lib/src/runtime.dart");

    assert!(
        apis.contains("ImageComputedParams? computedParams,"),
        "ImageApi.get/list must accept a typed ImageComputedParams under RPC transport: {apis}"
    );
    assert!(
        apis.contains("cratestackWithRpcComputedParams(input, computedParams?.toWire())"),
        "ImageApi.list must fold computedParams.toWire() into the RPC input frame: {apis}"
    );
    assert!(
        apis.contains("cratestackWithRpcComputedParams({'id': id}, computedParams?.toWire())"),
        "ImageApi.get must fold computedParams.toWire() into the RPC input frame: {apis}"
    );

    // The folding itself (JSON-encoding the already-`.toWire()`'d map
    // under the `computedParams` wire key) lives in the shared runtime
    // helper, not duplicated at each call site.
    assert!(
        runtime.contains("Map<String, Object?> cratestackWithRpcComputedParams("),
        "runtime.dart should define the shared RPC computedParams-folding helper: {runtime}"
    );
    assert!(
        runtime.contains("'computedParams': jsonEncode(computedParams),"),
        "cratestackWithRpcComputedParams should JSON-encode the already-toWire()'d params map \
         under the computedParams wire key: {runtime}"
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

/// Text-level proof that `builder`/`newBuilder`/`build`-named fields (the
/// round-1 static-`{Class}.builder()`-factory collision fixture — see
/// `tests/fixtures/builder_edge_cases.cstack`'s module doc for the history)
/// still come through as ordinary constructor fields, and that every data
/// class carries `@CratestackBuilder()`.
///
/// Issue #668 phase 2: this crate no longer emits a `{Class}Builder` class
/// at all (`package:cratestack_builder` does, from the annotation below),
/// so the collision this test used to guard against — a static
/// `{Class}.builder()` factory colliding with an instance field named
/// `builder` — can no longer arise on this crate's side; there is no
/// static factory left to collide. `dart-packages/cratestack_builder`'s
/// own `test/builder_generator_test.dart` (`no static builder() factory
/// is emitted`) covers that half now. The same is true of the required
/// `Json` field's `unnecessary_cast` concern — whether `build()` needs a
/// cast is now `dart-packages/cratestack_builder`'s own `castNeeded`
/// computation, not this crate's `FieldView::builder_cast_needed` (deleted
/// along with the rest of the builder-only derivation).
#[test]
fn builder_edge_case_fields_survive_as_ordinary_constructor_fields() {
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

    // No inline builder class survives at all — building a `Gadget`/
    // `CreateGadgetInput` fluently is entirely `package:cratestack_builder`'s
    // job now.
    assert!(
        !models.contains("Builder {"),
        "no inline `{{Class}}Builder` class should be emitted anymore, got:\n{models}"
    );

    // The `builder`/`newBuilder`/`build` fields survive as ordinary
    // constructor fields — proving the underlying defect class (a field
    // name colliding with a *generated* member) is structurally
    // impossible now that this crate generates no such member.
    assert!(
        models.contains("final String builder;") && models.contains("final String newBuilder;"),
        "expected `builder` and `newBuilder` to remain as ordinary Gadget fields, got:\n{models}"
    );
    assert!(
        models.contains("required this.builder,") && models.contains("required this.newBuilder,"),
        "expected `builder`/`newBuilder` as ordinary required constructor params on \
         CreateGadgetInput, got:\n{models}"
    );
    assert!(
        models.contains("required this.build,"),
        "expected the `build`-named field as an ordinary required constructor param on \
         CreateGadgetInput, got:\n{models}"
    );

    // Every data class this fixture produces carries the annotation —
    // `package:cratestack_builder`'s entry point.
    for class_name in [
        "Gadget",
        "CreateGadgetInput",
        "UpdateGadgetInput",
        "GadgetWhere",
        "GadgetOrderByClause",
        "GadgetFindMany",
    ] {
        let header = format!("\nclass {class_name} {{");
        let class_start = models.find(&header).unwrap_or_else(|| {
            panic!("generated output should declare `class {class_name}`, got:\n{models}")
        });
        let immediately_before = &models[..class_start];
        let annotation_line = immediately_before.lines().last().unwrap_or_default().trim();
        assert!(
            annotation_line.starts_with("@CratestackBuilder"),
            "expected `class {class_name}` to be immediately preceded by @CratestackBuilder(...), \
             got line `{annotation_line}` in:\n{models}"
        );
    }
}

/// Text-level proof of what's left of issue #661's Dart half on THIS
/// crate's side, now that `package:cratestack_builder` (not this crate)
/// derives everything about a `tags`-style list field's builder from the
/// emitted Dart source — the real running proof, including `flutter
/// analyze` and `dart run build_runner build`, lives in `just
/// verify-dart`'s `builder_edge_cases` fixture entry, which runs
/// `tests/fixtures/builder_edge_cases_list_test.dart` against the
/// generated-and-`build_runner`-expanded package (see `dart-packages/
/// cratestack_builder/test/builder_generator_test.dart` for the
/// generator's own unit coverage of the append-setter/default-empty-list
/// logic itself):
///
/// 1. `Gadget.tags`/`CreateGadgetInput.tags`/`UpdateGadgetInput.tags`'s
///    generated **constructors** are untouched — `required this.tags`
///    still appears verbatim on `CreateGadgetInput`, its `dart_type` is
///    still `List<String>`/`List<String>?` as before.
/// 2. `CreateGadgetInput`/`Gadget` (never `Patch`-kind) get
///    `@CratestackBuilder()` — `listDefaults` defaults to `true`, so an
///    unset `tags` still builds as `[]` once `cratestack_builder` expands
///    it.
/// 3. `UpdateGadgetInput` (`Patch`-kind) gets
///    `@CratestackBuilder(listDefaults: false)` instead — its unset `tags`
///    stays `null` once expanded, preserving the existing "never touched"
///    wire representation every other Patch field relies on.
#[test]
fn list_field_constructor_and_annotation_are_unaffected_by_moving_the_builder_out() {
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
    // `required` flag always produced.
    assert!(
        models.contains("final List<String> tags;") && models.contains("required this.tags,"),
        "CreateGadgetInput's constructor must keep requiring `tags`, got:\n{models}"
    );
    assert!(
        models.contains("final List<String>? tags;"),
        "Gadget/UpdateGadgetInput's constructor `tags` field must stay \
         nullable/optional, got:\n{models}"
    );

    // (2)/(3) `listDefaults` on the annotation, keyed off `DataClassKind`
    // exactly as before the builder itself moved out.
    assert!(
        models.contains("@CratestackBuilder()\nclass CreateGadgetInput {"),
        "CreateGadgetInput (Plain-kind) must get the default `listDefaults: true` \
         annotation, got:\n{models}"
    );
    assert!(
        models.contains("@CratestackBuilder()\nclass Gadget {"),
        "Gadget (ProjectionModel-kind) must get the default `listDefaults: true` \
         annotation too — issue #661 AC1 doesn't carve out model classes, got:\n{models}"
    );
    // `note String?` is a nullable Patch field, so `UpdateGadgetInput` also
    // carries `touchFlagFields: {'note'}` — see `patch_touch.rs`.
    assert!(
        models.contains(
            "@CratestackBuilder(listDefaults: false, touchFlagFields: {'note'})\nclass UpdateGadgetInput {"
        ),
        "UpdateGadgetInput (Patch-kind) must get `listDefaults: false` and `touchFlagFields: \
         {{'note'}}`, got:\n{models}"
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
