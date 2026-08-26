//! cratestack#743 — `@@internal("action")` route suppression, TypeScript
//! client generator coverage (`docs/design/route-suppression.md`).
//! Golden-file style assertions on rendered `client.ts`/`models.ts`: a
//! suppressed verb's method must be ABSENT (not present-but-403), for
//! the default (REST) generator, the `--swr` preset, and RPC transport,
//! and `Create<M>Input` must not be emitted once `create` is suppressed.

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

const SUPPRESSED_CREATE_REST: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
  @@internal("create")
}
"#;

const UNSUPPRESSED_REST: &str = r#"
model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
}
"#;

const SUPPRESSED_CREATE_RPC: &str = r#"
transport rpc

model Widget {
  id Int @id
  name String

  @@allow("read", auth() != null)
  @@allow("create", auth() != null)
  @@allow("update", auth() != null)
  @@allow("delete", auth() != null)
  @@internal("create")
}
"#;

fn config(swr: bool) -> TypeScriptGeneratorConfig {
    TypeScriptGeneratorConfig {
        package_name: "@example/widget-client".to_owned(),
        base_path: "/api".to_owned(),
        template_dir: None,
        swr,
        full_selection: false,
        refine: false,
        tanstack: false,
        schema_sha256: "deadbeef".to_owned(),
    }
}

fn package_file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| {
            panic!(
                "expected generated file `{file_name}`; got: {:?}",
                package
                    .files
                    .iter()
                    .map(|f| &f.file_name)
                    .collect::<Vec<_>>()
            )
        })
        .contents
        .as_str()
}

#[test]
fn default_rest_preset_omits_the_suppressed_create_method_and_input_type() {
    let schema = cratestack_parser::parse_schema(SUPPRESSED_CREATE_REST)
        .expect("fixture schema should parse");
    let package = generate_package(&schema, &config(false)).expect("template should render");
    let client = package_file(&package, "src/client.ts");
    let models = package_file(&package, "src/models.ts");

    assert!(
        !client.contains("create(input: CreateWidgetInput"),
        "suppressed create() must be absent from client.ts:\n{client}"
    );
    assert!(
        !models.contains("export interface CreateWidgetInput"),
        "suppressed create's input type must be absent from models.ts:\n{models}"
    );
    // Surviving verbs must stay present.
    assert!(client.contains("list(options: CratestackQueryRequestConfig"));
    assert!(client.contains("get(id: number"));
    assert!(client.contains("update("));
    assert!(client.contains("delete(id: number"));
    assert!(models.contains("export interface UpdateWidgetInput"));
}

/// Negative-control sibling, in the same file: with no `@@internal`
/// attribute at all, both the method and the input type are present —
/// proving the absence above is pinned to the attribute.
#[test]
fn default_rest_preset_emits_create_when_nothing_is_suppressed() {
    let schema =
        cratestack_parser::parse_schema(UNSUPPRESSED_REST).expect("fixture schema should parse");
    let package = generate_package(&schema, &config(false)).expect("template should render");
    let client = package_file(&package, "src/client.ts");
    let models = package_file(&package, "src/models.ts");

    assert!(client.contains("create(input: CreateWidgetInput"));
    assert!(models.contains("export interface CreateWidgetInput"));
}

#[test]
fn swr_preset_omits_the_suppressed_create_function_and_input_type() {
    let schema = cratestack_parser::parse_schema(SUPPRESSED_CREATE_REST)
        .expect("fixture schema should parse");
    let package = generate_package(&schema, &config(true)).expect("swr template should render");
    let widget_file = package_file(&package, "src/swr/models/widget.ts");
    let widget_hooks = package_file(&package, "src/swr/models/widget.hooks.ts");

    assert!(
        !widget_file.contains("export interface CreateWidgetInput"),
        "suppressed create's input type must be absent: {widget_file}"
    );
    assert!(
        !widget_file.contains("export async function createWidget("),
        "suppressed create function must be absent: {widget_file}"
    );
    assert!(
        !widget_hooks.contains("createWidget"),
        "suppressed create hook must not reference the omitted create fn: {widget_hooks}"
    );
    // Surviving verbs must stay present.
    assert!(widget_file.contains("export async function listWidgets("));
    assert!(widget_file.contains("export async function getWidget("));
    assert!(widget_file.contains("export async function updateWidget("));
    assert!(widget_file.contains("export async function deleteWidget("));
    assert!(widget_file.contains("export interface UpdateWidgetInput"));
}

#[test]
fn rpc_transport_omits_the_suppressed_create_method() {
    let schema = cratestack_parser::parse_schema(SUPPRESSED_CREATE_RPC)
        .expect("fixture schema should parse");
    let package = generate_package(&schema, &config(false)).expect("rpc template should render");
    let client = package_file(&package, "src/client.ts");
    let models = package_file(&package, "src/models.ts");

    assert!(
        !client.contains("create(input: CreateWidgetInput"),
        "suppressed RPC create() must be absent: {client}"
    );
    assert!(
        !models.contains("export interface CreateWidgetInput"),
        "suppressed create's input type must be absent: {models}"
    );
    assert!(client.contains("list(query: CratestackRpcListQuery"));
    assert!(client.contains("get(id: number"));
    assert!(client.contains("update("));
    assert!(client.contains("delete(id: number"));
}
