//! Ticket #210: Dart generator unit tests for `transport grpc` schemas,
//! mirroring `cratestack-client-typescript/tests/generator_grpc.rs`'s
//! pattern (and, in turn, `tests/generator.rs`'s). Two schema sources:
//! `examples/grpc-widgets/schemas/widgets.cstack` (ticket #171's real,
//! `grpcurl`-verified fixture, reused rather than re-invented) for the
//! file-set/basic-shape assertions, and a small inline schema with a
//! relation + a create-disabled model for the recursive message-collection
//! and `allows_create`-gating paths the widgets fixture (one flat model)
//! doesn't exercise.

use cratestack_client_dart::{DartGeneratorConfig, DartPreset, generate_package};
use cratestack_proto::PbLock;

fn widgets_schema() -> cratestack_core::Schema {
    cratestack_parser::parse_schema_file("../../examples/grpc-widgets/schemas/widgets.cstack")
        .expect("widgets fixture should parse")
}

fn widgets_lock() -> PbLock {
    let text = std::fs::read_to_string("../../examples/grpc-widgets/schemas/widgets.pb.lock")
        .expect("widgets lock should read");
    PbLock::from_toml(&text).expect("widgets lock should parse")
}

fn config_with_lock(pb_lock: PbLock) -> DartGeneratorConfig {
    DartGeneratorConfig {
        library_name: "widgets_grpc_client".to_owned(),
        base_path: "/".to_owned(),
        template_dir: None,
        preset: DartPreset::Default,
        pb_lock: Some(pb_lock),
        // gRPC is out of scope for the schema-fingerprint header (issue
        // #178) for this pass, matching the TypeScript gRPC-Web client's
        // own scope note — this value only needs to make the struct
        // literal compile; grpc-runtime.dart.j2 never reads it.
        schema_sha256: "unused-for-grpc".to_owned(),
        native_cbor: false,
    }
}

fn package_file<'a>(
    package: &'a cratestack_client_dart::GeneratedDartPackage,
    file_name: &str,
) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}

#[test]
fn generates_the_expected_file_set_for_a_grpc_schema() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("grpc schema should generate");

    assert_eq!(package.files.len(), 8, "{:#?}", package.files);
    for expected in [
        "pubspec.yaml",
        "README.md",
        "CHANGELOG.md",
        "analysis_options.yaml",
        "lib/src/models.dart",
        "lib/widgets_grpc_client.dart",
        "lib/src/runtime.dart",
        "lib/src/apis.dart",
    ] {
        package_file(&package, expected);
    }
}

#[test]
fn missing_pb_lock_is_a_hard_error_for_a_grpc_schema() {
    let error = generate_package(
        &widgets_schema(),
        &DartGeneratorConfig {
            pb_lock: None,
            ..config_with_lock(widgets_lock())
        },
    )
    .expect_err("grpc schema with no pb_lock should fail");

    assert!(matches!(
        error,
        cratestack_client_dart::DartGeneratorError::MissingPbLock
    ));
}

#[test]
fn pubspec_depends_on_package_grpc_and_not_on_the_rest_rpc_stack() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let pubspec = package_file(&package, "pubspec.yaml");

    assert!(pubspec.contains("grpc: ^5.1.0"), "{pubspec}");
    assert!(!pubspec.contains("dio:"), "{pubspec}");
    assert!(!pubspec.contains("cbor:"), "{pubspec}");
    assert!(!pubspec.contains("flutter_riverpod"), "{pubspec}");
}

#[test]
fn message_field_descriptors_use_the_real_lock_numbers() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let apis = package_file(&package, "lib/src/apis.dart");

    // Straight from `widgets.pb.lock`: `Widget.id = 1`, `Widget.name = 2`.
    assert!(
        apis.contains(
            "'Widget': [\n    CratestackGrpcFieldDescriptor(property: 'id', number: 1, kind: 'int64', repeated: false),\n    CratestackGrpcFieldDescriptor(property: 'name', number: 2, kind: 'string', repeated: false),\n  ],"
        ),
        "{apis}"
    );
    // `PageOfWidget.items` is a repeated message reference to `Widget`.
    assert!(
        apis.contains(
            "CratestackGrpcFieldDescriptor(property: 'items', number: 1, kind: 'message', repeated: true, refName: 'Widget'),"
        ),
        "{apis}"
    );
    // `PageInfo`'s bools use implicit presence (`defaultsWhenAbsent`).
    assert!(
        apis.contains(
            "CratestackGrpcFieldDescriptor(property: 'hasNextPage', number: 3, kind: 'bool', repeated: false, defaultsWhenAbsent: true),"
        ),
        "{apis}"
    );
}

#[test]
fn method_paths_use_the_locked_package_and_op_id_derived_method_name() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let apis = package_file(&package, "lib/src/apis.dart");

    assert!(apis.contains("'/widgets_api.Api/ModelWidgetList'"));
    assert!(apis.contains("'/widgets_api.Api/ModelWidgetGet'"));
    assert!(apis.contains("'/widgets_api.Api/ModelWidgetCreate'"));
    assert!(apis.contains("'/widgets_api.Api/ModelWidgetUpdate'"));
    assert!(apis.contains("'/widgets_api.Api/ModelWidgetDelete'"));
}

#[test]
fn client_class_exposes_a_model_accessor_and_crud_methods() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let apis = package_file(&package, "lib/src/apis.dart");

    assert!(
        apis.contains("class WidgetsGrpcClientCratestackClient {"),
        "{apis}"
    );
    assert!(
        apis.contains("late final WidgetApi widgets = WidgetApi(runtime);"),
        "{apis}"
    );
    assert!(apis.contains("class WidgetApi {"), "{apis}");
    assert!(apis.contains("Future<Page<Widget>> list(["), "{apis}");
    assert!(
        apis.contains("Future<Widget> get(int id, {CallOptions? options}) async {"),
        "{apis}"
    );
    assert!(
        apis.contains(
            "Future<Widget> create(CreateWidgetInput input, {CallOptions? options}) async {"
        ),
        "{apis}"
    );
    assert!(
        apis.contains("Future<Widget> update(\n    int id,\n    UpdateWidgetInput patch, {"),
        "{apis}"
    );
    assert!(
        apis.contains("Future<void> delete(int id, {CallOptions? options}) async {"),
        "{apis}"
    );
}

#[test]
fn runtime_declares_the_grpc_client_channel_wrapper_and_error_type() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let runtime = package_file(&package, "lib/src/runtime.dart");

    assert!(runtime.contains("import 'package:grpc/grpc.dart';"));
    assert!(runtime.contains("class CratestackGrpcRuntime extends Client {"));
    assert!(runtime.contains("factory CratestackGrpcRuntime.host("));
    assert!(
        runtime.contains("ChannelCredentials credentials = const ChannelCredentials.insecure(),")
    );
    assert!(runtime.contains("class CratestackGrpcError implements Exception {"));
    assert!(runtime.contains("'not_found'"));
    assert!(runtime.contains("'conflict'"));
    assert!(runtime.contains("Uint8List encodeMessage("));
    assert!(runtime.contains("CratestackValueMap decodeMessage("));
    // `Client._channel` is private, so a caller has no way to close the
    // connection unless the runtime exposes it itself (verified live:
    // without this, a script hangs on exit instead of terminating).
    assert!(runtime.contains("final ClientChannel channel;"));
    assert!(runtime.contains("Future<void> shutdown() => channel.shutdown();"));
    assert!(runtime.contains("Future<void> terminate() => channel.terminate();"));
}

#[test]
fn crud_methods_accept_per_call_options_not_just_a_runtime_default() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let apis = package_file(&package, "lib/src/apis.dart");

    // Every CRUD method takes its own `CallOptions?` so a caller can
    // supply per-request auth/deadline/metadata (matching the TypeScript
    // gRPC-Web client's per-call `options` parameter) instead of being
    // limited to one static default set at `CratestackGrpcRuntime`
    // construction time.
    assert!(apis.contains("Future<Page<Widget>> list([\n"), "{apis}");
    assert!(apis.contains("CratestackGrpcListInput input = const CratestackGrpcListInput(),\n    CallOptions? options,\n  ]) async {"), "{apis}");
    assert!(
        apis.contains("Future<Widget> get(int id, {CallOptions? options}) async {"),
        "{apis}"
    );
    assert!(
        apis.contains(
            "Future<Widget> create(CreateWidgetInput input, {CallOptions? options}) async {"
        ),
        "{apis}"
    );
    assert!(
        apis.contains("Future<Widget> update(\n    int id,\n    UpdateWidgetInput patch, {\n    CallOptions? options,\n  }) async {"),
        "{apis}"
    );
    assert!(
        apis.contains("Future<void> delete(int id, {CallOptions? options}) async {"),
        "{apis}"
    );
    assert!(apis.contains("import 'package:grpc/grpc.dart';"));
}

/// A relation field (`Post.author: Author`) and a create-disabled model
/// (`Author` allows only `read`) — neither exists in the flat
/// `widgets.cstack` fixture. Exercises [1] the recursive message
/// collection following a message-typed field reference, and [2] the
/// `model_allows_create` gate on `Create<M>Input` collection (mirrors the
/// TypeScript gRPC-Web generator's own regression test for this exact
/// schema shape).
#[test]
fn relation_fields_recurse_and_create_disabled_models_skip_create_input() {
    let source = r#"
transport grpc

datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Author {
  id Int @id
  name String

  @@allow("read", auth() != null)
}

model Post {
  id Int @id
  title String
  authorId Int
  author Author @relation(fields: [authorId], references: [id])

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
}
"#;
    let schema = cratestack_parser::parse_schema(source).expect("inline schema should parse");
    let extra = cratestack_proto::synthesize_messages(&schema).expect("should synthesize");
    let mut lock =
        cratestack_proto::build_lock(&schema, None, &extra).expect("should build a fresh lock");
    lock.package = Some("relation_pkg".to_owned());

    let package = generate_package(&schema, &config_with_lock(lock)).expect("should generate");
    let apis = package_file(&package, "lib/src/apis.dart");

    // `Post`'s message descriptor references `Author` as a nested message
    // — proves the recursive collector followed the relation.
    assert!(apis.contains("refName: 'Author'"), "{apis}");
    assert!(apis.contains("'Author': [\n"), "{apis}");

    // `Author` disallows create: no `CreateAuthorInput` message, no
    // `.create()` method taking one.
    assert!(!apis.contains("CreateAuthorInput"), "{apis}");
    assert!(apis.contains("class AuthorApi {"), "{apis}");
    // `Post` allows create: `CreatePostInput` exists and `PostApi` gets a
    // `.create()`.
    assert!(apis.contains("'CreatePostInput': [\n"), "{apis}");
    assert!(
        apis.contains("Future<Post> create(CreatePostInput input, {CallOptions? options}) async {"),
        "{apis}"
    );
}
