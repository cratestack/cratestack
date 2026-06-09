use cratestack_client_dart::{DartGeneratorConfig, generate_package};
use cratestack_parser::parse_schema;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
}

#[test]
fn preserves_custom_fields_on_generated_types() {
    let schema = cratestack_parser::parse_schema_file(
        "../cratestack-pg/tests/fixtures/custom_fields.cstack",
    )
    .expect("fixture schema should parse");

    let package = generate_package(&schema, &DartGeneratorConfig::default())
        .expect("default template should render");
    let models = package_file(&package, "lib/src/models.dart");

    assert!(models.contains("class Image {"));
    assert!(models.contains("required this.thumbnailUrl,"));
    assert!(models.contains("final String thumbnailUrl;"));
    assert!(models.contains("'thumbnailUrl': thumbnailUrl,"));
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
