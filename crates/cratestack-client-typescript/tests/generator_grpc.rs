//! Ticket #172: TypeScript generator unit tests for `transport grpc`
//! schemas, mirroring `tests/generator.rs`'s pattern. Two schema sources:
//! `examples/grpc-widgets/schemas/widgets.cstack` (ticket #171's real,
//! `grpcurl`-verified fixture — reused rather than re-invented, per this
//! ticket's brief) for the file-set/basic-shape assertions, and a small
//! inline schema with a relation + a create-disabled model for the
//! recursive message-collection and `allows_create`-gating paths the
//! widgets fixture (one flat model) doesn't exercise.

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};
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

fn config_with_lock(pb_lock: PbLock) -> TypeScriptGeneratorConfig {
    TypeScriptGeneratorConfig {
        package_name: "@example/widgets-grpc".to_owned(),
        base_path: "/".to_owned(),
        template_dir: None,
        full_selection: false,
        pb_lock: Some(pb_lock),
        // gRPC-Web is out of scope for the schema-fingerprint header (issue
        // #178) — this crate's TypeScriptGeneratorConfig::schema_sha256 doc
        // comment states REST/RPC only. This value only needs to make the
        // struct literal compile; grpc-web-runtime.ts.j2 never reads it.
        schema_sha256: "unused-for-grpc-web".to_owned(),
    }
}

fn package_file<'a>(
    package: &'a cratestack_client_typescript::GeneratedTypeScriptPackage,
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

    // Same 4 common files as every transport, plus 4 gRPC-Web-specific
    // ones (runtime/client/react-query/index) — no queries.ts (REST-only)
    // and no procedure surface (gRPC never routes procedures, ticket
    // #171).
    assert_eq!(package.files.len(), 8, "{:#?}", package.files);
    for expected in [
        "package.json",
        "tsconfig.json",
        "README.md",
        "src/models.ts",
        "src/runtime.ts",
        "src/client.ts",
        "src/react-query.ts",
        "src/index.ts",
    ] {
        package_file(&package, expected);
    }
}

#[test]
fn missing_pb_lock_is_a_hard_error_for_a_grpc_schema() {
    let error = generate_package(
        &widgets_schema(),
        &TypeScriptGeneratorConfig {
            pb_lock: None,
            ..config_with_lock(widgets_lock())
        },
    )
    .expect_err("grpc schema with no pb_lock should fail");

    assert!(matches!(
        error,
        cratestack_client_typescript::TypeScriptGeneratorError::MissingPbLock
    ));
}

#[test]
fn full_selection_still_produces_required_model_fields_for_grpc() {
    let mut config = config_with_lock(widgets_lock());
    config.full_selection = true;
    let package = generate_package(&widgets_schema(), &config).expect("should generate");
    let models = package_file(&package, "src/models.ts");

    // `--full-selection` uses `InterfaceKind::Plain` instead of `Model`
    // (`context::build_template_context`, unchanged by this ticket) —
    // fields follow the schema's own required/optional split rather than
    // being forced optional for partial-selection. Both `Widget` fields
    // are schema-required, so both should be non-optional here.
    assert!(
        models.contains("export interface Widget {\n  id: number;\n  name: string;\n}"),
        "{models}"
    );
}

#[test]
fn default_generation_forces_every_model_field_optional_for_grpc() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let models = package_file(&package, "src/models.ts");

    assert!(
        models.contains("export interface Widget {\n  id?: number;\n  name?: string;\n}"),
        "{models}"
    );
}

#[test]
fn message_field_descriptors_use_the_real_lock_numbers() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let client = package_file(&package, "src/client.ts");

    // Straight from `widgets.pb.lock`: `Widget.id = 1`, `Widget.name = 2`.
    assert!(client.contains(
        r#"Widget: [
    { property: "id", number: 1, kind: "int64", repeated: false },
    { property: "name", number: 2, kind: "string", repeated: false },
  ]"#
    ));
    // `PageOfWidget.items` is a repeated message reference to `Widget`.
    assert!(client.contains(
        r#"{ property: "items", number: 1, kind: "message", repeated: true, refName: "Widget" }"#
    ));
    // `PageInfo`'s bools use implicit presence (`defaultsWhenAbsent`).
    assert!(client.contains(
        r#"{ property: "hasNextPage", number: 3, kind: "bool", repeated: false, defaultsWhenAbsent: true }"#
    ));
}

#[test]
fn method_paths_use_the_locked_package_and_op_id_derived_method_name() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let client = package_file(&package, "src/client.ts");

    assert!(client.contains(r#""/widgets_api.Api/ModelWidgetList""#));
    assert!(client.contains(r#""/widgets_api.Api/ModelWidgetGet""#));
    assert!(client.contains(r#""/widgets_api.Api/ModelWidgetCreate""#));
    assert!(client.contains(r#""/widgets_api.Api/ModelWidgetUpdate""#));
    assert!(client.contains(r#""/widgets_api.Api/ModelWidgetDelete""#));
}

#[test]
fn no_procedure_surface_is_generated_for_grpc() {
    let package = generate_package(&widgets_schema(), &config_with_lock(widgets_lock()))
        .expect("should generate");
    let client = package_file(&package, "src/client.ts");
    let react_query = package_file(&package, "src/react-query.ts");

    assert!(!client.contains("ProceduresApi"));
    assert!(!react_query.contains("Procedure"));
}

/// A relation field (`Post.author: Author`) and a create-disabled model
/// (`Author` allows only `read`) — neither exists in the flat
/// `widgets.cstack` fixture. Exercises [1] the recursive message
/// collection following a message-typed field reference, and [2] the
/// `model_allows_create` gate on `Create<M>Input` collection (a real bug
/// caught by exactly this schema shape during development: collecting
/// `CreateAuthorInput` unconditionally looked up a lock entry that
/// `cratestack-proto` never assigns for a create-disabled model).
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
    let client = package_file(&package, "src/client.ts");

    // `Post`'s message descriptor references `Author` as a nested message
    // — proves the recursive collector followed the relation.
    assert!(client.contains(r#"refName: "Author""#), "{client}");
    assert!(client.contains("Author: [\n"), "{client}");

    // `Author` disallows create: no `CreateAuthorInput` message, no
    // `.create()` method taking one.
    assert!(!client.contains("CreateAuthorInput"), "{client}");
    assert!(client.contains("class AuthorApi {"), "{client}");
    // `Post` allows create: `CreatePostInput` exists and `PostApi` gets a
    // `.create()`.
    assert!(client.contains("CreatePostInput: [\n"), "{client}");
    assert!(
        client.contains("async create(input: CreatePostInput"),
        "{client}"
    );
}
