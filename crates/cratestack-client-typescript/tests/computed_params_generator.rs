//! Static (CI-safe, no external tooling) coverage for the typed,
//! per-model-gated `computedParams` surface (`docs/design/computed-fields.md`
//! stage 4) — `tests/computed_params_typed_gate_tsc.rs` is the real-`tsc`
//! proof the gate is actually enforced; this file asserts the generated
//! source itself has the right shape, and (the cache-key fix this stage
//! also lands) that the RPC `swr` preset's `get` cache key incorporates
//! `computedParams`, not just `id`.
//!
//! Uses `tests/fixtures/computed_params.cstack` (REST) and
//! `tests/fixtures/computed_params_rpc.cstack` (RPC): both declare a
//! gated model (`Image`, with a parameterized `@computed(params:
//! ProxyParams?)` field) and an ungated one (`Widget`, no computed fields
//! at all), so every assertion below can show both the positive (gated)
//! and negative (ungated) side in one generation run.

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

#[test]
fn rest_gated_model_gets_a_typed_computed_params_interface_and_query_config() {
    let package = generate_for("computed_params", false);
    let models = file(&package, "src/models.ts");
    let client = file(&package, "src/client.ts");

    assert!(
        models.contains("export interface ImageComputedParams {\n  proxyUrl?: ProxyParams;\n}"),
        "models.ts must declare the generated ImageComputedParams interface: {models}"
    );
    // Widget has no computed fields at all, so it must never get a
    // `WidgetComputedParams` interface.
    assert!(
        !models.contains("WidgetComputedParams"),
        "models.ts must not declare a computed-params interface for an ungated model: {models}"
    );

    for method in [
        "list(options: CratestackQueryRequestConfig<ImageComputedParams> = {})",
        "get(id: number, options: CratestackQueryRequestConfig<ImageComputedParams> = {})",
    ] {
        assert!(
            client.contains(method),
            "client.ts's ImageApi must instantiate {method:?}: {client}"
        );
    }
    // Widget (ungated) must keep the bare, un-instantiated generic —
    // relying on `CratestackQueryRequestConfig`'s own `= never` default.
    assert!(
        client.contains("list(options: CratestackQueryRequestConfig = {}): Promise<Widget[]>"),
        "client.ts's WidgetApi.list must stay ungated (bare CratestackQueryRequestConfig): {client}"
    );
    assert!(
        !client.contains("CratestackQueryRequestConfig<WidgetComputedParams>"),
        "client.ts must never instantiate a computed-params generic for an ungated model: {client}"
    );
}

#[test]
fn rpc_gated_model_gets_typed_list_query_and_get_options() {
    let package = generate_for("computed_params_rpc", false);
    let client = file(&package, "src/client.ts");

    assert!(
        client.contains("export interface ImageApiGetOptions extends CratestackRpcCallOptions"),
        "client.ts must declare ImageApiGetOptions for the gated model: {client}"
    );
    assert!(
        client.contains("computedParams?: ImageComputedParams;"),
        "ImageApiGetOptions must carry a typed computedParams: {client}"
    );
    assert!(
        client.contains(
            "list(query: CratestackRpcListQuery<ImageComputedParams> = {}, options: CratestackRpcCallOptions = {})"
        ),
        "client.ts's ImageApi.list must instantiate CratestackRpcListQuery<ImageComputedParams>: {client}"
    );
    assert!(
        client.contains("get(id: number, options: ImageApiGetOptions = {})"),
        "client.ts's ImageApi.get must accept ImageApiGetOptions: {client}"
    );

    // Widget (ungated) keeps the original, non-generic shape.
    assert!(
        client.contains(
            "list(query: CratestackRpcListQuery = {}, options: CratestackRpcCallOptions = {})"
        ),
        "client.ts's WidgetApi.list must stay ungated: {client}"
    );
    assert!(
        client.contains("get(id: number, options: CratestackRpcCallOptions = {})"),
        "client.ts's WidgetApi.get must stay ungated: {client}"
    );
    assert!(
        !client.contains("WidgetApiGetOptions"),
        "client.ts must never declare a GetOptions type for an ungated model: {client}"
    );
}

/// The cache-key correctness fix this stage also lands: the RPC `swr`
/// preset's per-model `get` cache key must incorporate `computedParams`,
/// not just `id` — two reads of the same id with different resolver
/// params are different responses, so omitting it collides them in the
/// swr cache. `tests/swr_generator.rs` covers the rest of `--swr`'s file
/// set/content; this is scoped to the one line this stage changed.
#[test]
fn rpc_swr_get_cache_key_incorporates_computed_params_on_a_gated_model() {
    let package = generate_for("computed_params_rpc", true);
    let keys = file(&package, "src/swr/swr-keys.ts");

    assert!(
        keys.contains(
            "get: (id: number | null | undefined, computedParams?: ImageComputedParams) =>\n        \
             id == null ? null : ([\"model.Image.get\", id, computedParams] as const),"
        ),
        "swr-keys.ts's Image.get key must incorporate computedParams: {keys}"
    );
    // Widget (ungated) keeps the original id-only key — there is no
    // computedParams to key on, since the server would 422 it anyway.
    assert!(
        keys.contains(
            "get: (id: number | null | undefined) =>\n        id == null ? null : ([\"model.Widget.get\", id] as const),"
        ),
        "swr-keys.ts's Widget.get key must stay id-only (ungated): {keys}"
    );
}

fn generate_for(fixture_stem: &str, swr: bool) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "computed-params-fixture-client".to_owned(),
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: generation should succeed: {error}"))
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, file_name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == file_name)
        .unwrap_or_else(|| panic!("missing generated file {file_name}"))
        .contents
        .as_str()
}
