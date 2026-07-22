// Golden-file snapshot tests for the TypeScript generator. Two
// fixtures cover both code paths:
//
//   * tiny_rest.cstack — default transport (REST). Generator emits
//     the fetch-based runtime + REST-shaped API classes.
//   * tiny_rpc.cstack  — `transport rpc`. Generator emits the
//     CratestackRpcRuntime + API classes calling
//     `runtime.call('model.<Name>.<verb>', input)`.
//
// To update the snapshots after intentional changes, run with
// `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-typescript`.

use std::fs;
use std::path::{Path, PathBuf};

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

#[test]
fn rest_snapshot_matches_fixture() {
    run_snapshot("tiny_rest", "tiny-rest-client");
}

#[test]
fn rpc_snapshot_matches_fixture() {
    run_snapshot("tiny_rpc", "tiny-rpc-client");
}

#[test]
fn rpc_client_invokes_runtime_call_with_dotted_op_ids() {
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let client = package_file(&package, "src/client.ts");
    // Op ids must match the format the server-side macro emits.
    assert!(
        client.contains("\"model.Widget.list\""),
        "client.ts is missing the `model.Widget.list` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.get\""),
        "client.ts is missing the `model.Widget.get` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.create\""),
        "client.ts is missing the `model.Widget.create` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.update\""),
        "client.ts is missing the `model.Widget.update` op id:\n{client}"
    );
    assert!(
        client.contains("\"model.Widget.delete\""),
        "client.ts is missing the `model.Widget.delete` op id:\n{client}"
    );
    assert!(
        client.contains("\"procedure.echoName\""),
        "client.ts is missing the `procedure.echoName` op id:\n{client}"
    );
}

#[test]
fn rpc_runtime_exports_rpc_error_class() {
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("class CratestackRpcError"),
        "runtime.ts must define CratestackRpcError"
    );
    assert!(
        runtime.contains("RpcErrorCode"),
        "runtime.ts must define the RpcErrorCode union"
    );
    assert!(
        runtime.contains("\"not_found\""),
        "runtime.ts must include the `not_found` RPC code"
    );
    assert!(
        runtime.contains("\"unauthenticated\""),
        "runtime.ts must include the `unauthenticated` RPC code"
    );
}

#[test]
fn rpc_runtime_exposes_pluggable_codec_option() {
    // Regression test for #125: the generated RPC runtime used to
    // hardcode "application/json" as both Content-Type and Accept in
    // call()/batch()/stream(), with no way for a consumer whose backend
    // defaults to CBOR to plug in a different codec.
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("export interface CratestackRpcCodec"),
        "runtime.ts must define a CratestackRpcCodec extension point"
    );
    assert!(
        runtime.contains("export const jsonRpcCodec: CratestackRpcCodec"),
        "runtime.ts must export a default jsonRpcCodec"
    );
    assert!(
        runtime.contains("codec?: CratestackRpcCodec;"),
        "CratestackRpcClientOptions must accept a codec override"
    );
    assert!(
        runtime.contains("this.codec = options.codec ?? jsonRpcCodec;"),
        "runtime must default to jsonRpcCodec when no codec option is supplied"
    );
    assert!(
        runtime.contains("headers.set(\"Accept\", this.codec.contentType);")
            && runtime.contains("headers.set(\"Content-Type\", this.codec.contentType);"),
        "call()/batch()/stream() must derive Accept/Content-Type from the configured codec"
    );
    assert_eq!(
        runtime.matches("\"application/json\"").count(),
        1,
        "\"application/json\" must appear exactly once — as jsonRpcCodec's own \
         contentType literal — not hardcoded again in call()/batch()/stream():\n{runtime}"
    );
}

#[test]
fn generated_rpc_runtime_satisfies_exact_optional_property_types() {
    // Regression test: the generator's own tsconfig.json.j2 sets
    // exactOptionalPropertyTypes, but the generated runtime didn't
    // actually satisfy it — `this.defaultHeaders = options.headers;`
    // and three `signal: options.signal,` fetch-options fields all
    // failed a real `tsc --noEmit` run under that config (verified
    // manually; this repo has no tsc-in-CI harness, so this test
    // pins the source patterns that were confirmed to fix it).
    let package = generate_for("tiny_rpc", "tiny-rpc-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains(
            "readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;"
        ),
        "defaultHeaders must explicitly include `| undefined` in its type so assigning a \
         possibly-undefined value type-checks under exactOptionalPropertyTypes:\n{runtime}"
    );
    assert!(
        !runtime.contains("signal: options.signal,"),
        "fetch()'s RequestInit.signal is `AbortSignal | null` (no `| undefined`) — passing \
         `options.signal` directly fails under exactOptionalPropertyTypes; must coalesce to null:\n{runtime}"
    );
    assert_eq!(
        runtime.matches("signal: options.signal ?? null,").count(),
        3,
        "call()/batch()/stream() should all coalesce signal to null:\n{runtime}"
    );
}

#[test]
fn generated_rest_runtime_satisfies_exact_optional_property_types() {
    let package = generate_for("tiny_rest", "tiny-rest-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains(
            "readonly defaultHeaders: HeadersInit | (() => HeadersInit | Promise<HeadersInit>) | undefined;"
        ),
        "defaultHeaders must explicitly include `| undefined`:\n{runtime}"
    );
    assert!(
        runtime.contains("body: body ?? null,")
            && runtime.contains("signal: options.signal ?? null,"),
        "fetch()'s RequestInit.body/signal are typed without `| undefined` — must coalesce to null:\n{runtime}"
    );
    assert!(
        runtime.contains("headers?: HeadersInit | undefined;")
            && runtime.contains("query?: Record<string, unknown> | undefined;")
            && runtime.contains("signal?: AbortSignal | undefined;"),
        "CratestackRequestOptions' fields must explicitly include `| undefined` — otherwise \
         generated client.ts methods that spread `{{ headers: options.headers, ... }}` fail to \
         type-check under exactOptionalPropertyTypes, since the source config types \
         (CratestackRequestConfig etc.) are themselves optional-without-undefined:\n{runtime}"
    );
}

#[test]
fn rest_client_keeps_rest_style_methods() {
    let package = generate_for("tiny_rest", "tiny-rest-client");
    let runtime = package_file(&package, "src/runtime.ts");
    assert!(
        runtime.contains("class CratestackRuntime"),
        "REST runtime must keep the existing CratestackRuntime class"
    );
    let client = package_file(&package, "src/client.ts");
    assert!(
        client.contains("this.runtime.get<"),
        "REST client must keep using runtime.get<...>"
    );
    assert!(
        client.contains("this.runtime.post<"),
        "REST client must keep using runtime.post<...>"
    );
    // The REST client should NOT reference the RPC URL space.
    assert!(
        !client.contains("/rpc/"),
        "REST client should not reference /rpc/ URLs"
    );
}

#[test]
fn full_selection_emits_fully_required_model_interface() {
    // tiny_rest.cstack's Widget has a mix of required (`id`, `name`) and
    // nullable (`weight Int?`) scalars — exactly the mix the flag needs to
    // tell apart from projection-driven optionality.
    let package = generate_for_full_selection("tiny_rest", "tiny-rest-full-client");
    let models = package_file(&package, "src/models.ts");
    assert!(
        models.contains("export interface Widget {\n  id: number;\n  name: string;\n  weight?: number | null;\n}"),
        "--full-selection should require id/name (non-nullable in the schema) and keep weight \
         optional (nullable in the schema), rather than forcing every field optional:\n{models}"
    );
}

#[test]
fn full_selection_does_not_change_default_generation() {
    let default_package = generate_for("tiny_rest", "tiny-rest-client");
    let default_models = package_file(&default_package, "src/models.ts");
    assert!(
        default_models.contains("export interface Widget {\n  id?: number;\n  name?: string;\n  weight?: number | null;\n}"),
        "default (no flag) generation must keep every scalar field optional:\n{default_models}"
    );
}

#[test]
fn full_selection_leaves_create_and_update_inputs_unchanged() {
    // The flag targets read-model interfaces only — Create/Update inputs
    // already derive optionality from schema nullability (Create) or are
    // inherently partial by PATCH semantics (Update), so they must be
    // byte-identical with or without the flag.
    let default_package = generate_for("tiny_rest", "tiny-rest-client");
    let full_package = generate_for_full_selection("tiny_rest", "tiny-rest-full-client");
    let default_models = package_file(&default_package, "src/models.ts");
    let full_models = package_file(&full_package, "src/models.ts");

    for interface in ["CreateWidgetInput", "UpdateWidgetInput", "EchoNameArgs"] {
        let default_block = extract_interface_block(default_models, interface);
        let full_block = extract_interface_block(full_models, interface);
        assert_eq!(
            default_block, full_block,
            "{interface} should be unaffected by --full-selection"
        );
    }
}

#[test]
fn rest_and_rpc_share_models_ts() {
    let rest = generate_for("tiny_rest", "tiny-rest-client");
    let rpc = generate_for("tiny_rpc", "tiny-rpc-client");
    let rest_models = package_file(&rest, "src/models.ts");
    let rpc_models = package_file(&rpc, "src/models.ts");
    assert_eq!(
        rest_models, rpc_models,
        "models.ts should be identical across transports"
    );
}

fn run_snapshot(fixture_stem: &str, package_name: &str) {
    let package = generate_for(fixture_stem, package_name);
    let snapshot_dir = snapshot_root().join(fixture_stem);
    if std::env::var_os("CRATESTACK_UPDATE_SNAPSHOTS").is_some() {
        write_snapshot(&snapshot_dir, &package);
        return;
    }
    assert_snapshot_matches(&snapshot_dir, &package);
}

fn generate_for(fixture_stem: &str, package_name: &str) -> GeneratedTypeScriptPackage {
    generate_for_with_config(fixture_stem, package_name, false)
}

fn generate_for_full_selection(
    fixture_stem: &str,
    package_name: &str,
) -> GeneratedTypeScriptPackage {
    generate_for_with_config(fixture_stem, package_name, true)
}

fn generate_for_with_config(
    fixture_stem: &str,
    package_name: &str,
    full_selection: bool,
) -> GeneratedTypeScriptPackage {
    let fixture_path = fixture_root().join(format!("{fixture_stem}.cstack"));
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            base_path: "/api".to_owned(),
            template_dir: None,
            full_selection,
        },
    )
    .expect("default template should render")
}

fn write_snapshot(dir: &Path, package: &GeneratedTypeScriptPackage) {
    // Wipe the snapshot tree so deleted files don't linger.
    if dir.exists() {
        fs::remove_dir_all(dir).expect("snapshot dir should be removable");
    }
    fs::create_dir_all(dir).expect("snapshot dir should be creatable");
    for file in &package.files {
        let path = dir.join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("snapshot subdir should be creatable");
        }
        fs::write(&path, file.contents.as_bytes()).expect("snapshot file should write");
    }
}

fn assert_snapshot_matches(dir: &Path, package: &GeneratedTypeScriptPackage) {
    assert!(
        dir.exists(),
        "snapshot directory {dir:?} is missing — run `CRATESTACK_UPDATE_SNAPSHOTS=1 cargo test -p cratestack-client-typescript` to create it"
    );
    for file in &package.files {
        let path = dir.join(&file.file_name);
        let expected = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "snapshot file {path:?} is missing — run with CRATESTACK_UPDATE_SNAPSHOTS=1 to create it ({error})"
            )
        });
        assert_eq!(
            file.contents, expected,
            "snapshot mismatch for {} — run CRATESTACK_UPDATE_SNAPSHOTS=1 to refresh",
            file.file_name
        );
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshot_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn extract_interface_block<'a>(models: &'a str, interface_name: &str) -> &'a str {
    let start_marker = format!("export interface {interface_name} {{");
    let start = models
        .find(&start_marker)
        .unwrap_or_else(|| panic!("missing interface {interface_name} in:\n{models}"));
    let end = models[start..]
        .find("\n}")
        .map(|offset| start + offset + "\n}".len())
        .unwrap_or_else(|| panic!("unterminated interface {interface_name} in:\n{models}"));
    &models[start..end]
}

fn package_file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
