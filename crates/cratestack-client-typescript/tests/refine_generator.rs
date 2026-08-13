//! `--refine` (issue #571): the `@cratestack/refine` `ResourceMap` this
//! generator emits from the schema instead of asking consumers to
//! hand-write it.
//!
//! These are source-level assertions — they check that the right facts
//! reach the emitted file, and that the flag stays additive. The
//! complementary proof that the emitted file *type-checks* against
//! `@cratestack/refine`'s real `ResourceConfig` lives on the JS side,
//! where the real package is: `packages/cratestack-refine`'s
//! `tsconfig.typecheck.json`, run by its own `test` script in CI's
//! `js (@cratestack/refine)` job. Neither substitutes for the other —
//! a substring assertion cannot tell "satisfies the interface" from
//! "contains the right words".

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, TypeScriptGeneratorError,
    generate_package,
};

const REFINE_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

auth Operator {
  id Int
}

model Widget {
  id Int @id
  name String
  @@allow("read", auth() != null)
}

model Ledger {
  id Int @id
  label String
  revision Int @version
  @@paged
  @@allow("read", auth() != null)
}

model Product {
  sku String @id
  name String
  @@allow("read", auth() != null)
}
"#;

#[test]
fn refine_flag_emits_the_resource_map_with_the_schemas_own_facts() {
    let package = generate(REFINE_SCHEMA, true);
    let refine = file(&package, "src/refine.ts");

    // Widget: `@id` is `id`, no `@@paged`, no `@version`.
    assert!(
        refine.contains(
            "\"widgets\": {\n      api: client.widgets,\n      primaryKey: \"id\",\n      paged: false,\n    },"
        ),
        "widgets entry is wrong:\n{refine}"
    );
    // Ledger: `@@paged`, and a `@version` field that is NOT called
    // `version` — a hardcoded field name would pass a laxer assertion.
    assert!(
        refine.contains(
            "\"ledgers\": {\n      api: client.ledgers,\n      primaryKey: \"id\",\n      paged: true,\n      versionField: \"revision\",\n    },"
        ),
        "ledgers entry is wrong:\n{refine}"
    );
    // Product: `@id` is `sku`. refine assumes `id`, so getting this from
    // the schema rather than defaulting is the whole point.
    assert!(
        refine.contains("primaryKey: \"sku\","),
        "products entry should carry the non-`id` primary key:\n{refine}"
    );
}

/// `versionField` must be *absent*, not `undefined`. `ResourceConfig` is
/// consumed under `exactOptionalPropertyTypes`, where an explicit
/// `versionField: undefined` is a type error rather than an omission.
#[test]
fn a_model_without_version_omits_the_key_entirely() {
    let package = generate(REFINE_SCHEMA, true);
    let refine = file(&package, "src/refine.ts");
    let widgets = refine
        .split("\"widgets\": {")
        .nth(1)
        .and_then(|rest| rest.split("},").next())
        .expect("widgets entry should be present");
    assert!(
        !widgets.contains("versionField"),
        "widgets has no @version, so it must not mention versionField at all:\n{widgets}"
    );
    assert_eq!(
        refine.matches("versionField").count(),
        1,
        "exactly one of the three models declares @version:\n{refine}"
    );
}

#[test]
fn the_flag_is_additive_every_other_file_is_byte_identical() {
    let plain = generate(REFINE_SCHEMA, false);
    let with_refine = generate(REFINE_SCHEMA, true);

    assert!(
        !plain.files.iter().any(|f| f.file_name == "src/refine.ts"),
        "src/refine.ts must not be emitted without the flag"
    );

    for file in &plain.files {
        let counterpart = with_refine
            .files
            .iter()
            .find(|candidate| candidate.file_name == file.file_name)
            .unwrap_or_else(|| panic!("--refine dropped {}", file.file_name));
        // `package.json` and `src/index.ts` legitimately differ (the peer
        // dependency and the re-export); nothing else may.
        if matches!(file.file_name.as_str(), "package.json" | "src/index.ts") {
            continue;
        }
        assert_eq!(
            file.contents, counterpart.contents,
            "--refine changed {} — it must only ADD a file",
            file.file_name
        );
    }
}

#[test]
fn the_flag_declares_the_dependency_and_re_exports_the_module() {
    let with_refine = generate(REFINE_SCHEMA, true);
    let package_json = file(&with_refine, "package.json");
    // Both halves matter: the peer declares the consumer's obligation,
    // the dev dep is what makes `npm install && tsc` work in the
    // generated package on its own.
    assert_eq!(
        package_json.matches("\"@cratestack/refine\"").count(),
        2,
        "expected @cratestack/refine in both peerDependencies and devDependencies:\n{package_json}"
    );
    // The generated refine.ts only imports a *type* from
    // @cratestack/refine, but that type's declaration file imports
    // @refinedev/core — so without this, the generated package cannot
    // type-check on a clean install.
    assert!(
        package_json.contains("\"@refinedev/core\""),
        "@cratestack/refine's own .d.ts imports @refinedev/core:\n{package_json}"
    );
    assert!(
        file(&with_refine, "src/index.ts").contains("export * from \"./refine.js\";"),
        "index.ts should re-export the generated manifest"
    );
}

#[test]
fn refine_is_rejected_for_grpc_which_has_no_provider_to_bind_to() {
    // The gRPC-Web client speaks typed protobuf with no URL-query shaping
    // at all, and `@cratestack/refine` ships no provider for that shape —
    // an emitted refine.ts would have nothing to `tsc` against, so the
    // generator refuses instead. (REST and RPC are both supported — see
    // `refine_supports_rpc_schemas_with_the_same_per_resource_facts_as_rest`
    // below.)
    let grpc_schema = REFINE_SCHEMA.replace("datasource db {", "transport grpc\n\ndatasource db {");
    let error = try_generate(&grpc_schema, true, false)
        .expect_err("--refine on a gRPC-Web schema should be rejected");
    assert!(
        matches!(error, TypeScriptGeneratorError::RefineRequiresRestOrRpc),
        "expected RefineRequiresRestOrRpc, got: {error}"
    );
    // The same schema without the flag fails too (this fixture has no
    // `.pb.lock`, which `transport grpc` needs regardless of `--refine` —
    // see `generator_grpc.rs`), but for its own reason, not this one: the
    // rejection above is scoped to the flag, not baked into a schema that
    // categorically cannot generate.
    let error_without_flag = try_generate(&grpc_schema, false, false)
        .expect_err("this fixture has no .pb.lock, so a plain grpc generation fails too");
    assert!(
        !matches!(
            error_without_flag,
            TypeScriptGeneratorError::RefineRequiresRestOrRpc
        ),
        "without --refine, the failure must not be the refine-specific one: {error_without_flag}"
    );
}

/// Issue #591: `--preset swr` used to make `--refine` outright reject the
/// combination (`RefineUnsupportedPreset`) because the swr layout
/// *replaced* the default one, leaving no client class for a
/// `ResourceConfig` to bind to. `--swr` is additive now — the default
/// layout (and its client class) is always emitted regardless — so the
/// two compose freely: `--refine --swr` succeeds and emits both
/// `src/refine.ts` (bound to the always-present client class) and the
/// `src/swr/**` subtree.
#[test]
fn refine_and_swr_compose_without_conflict() {
    let package =
        try_generate(REFINE_SCHEMA, true, true).expect("--refine --swr should compose cleanly");
    assert!(
        file_named(&package, "src/refine.ts").is_some(),
        "--refine should still emit src/refine.ts alongside --swr"
    );
    assert!(
        file_named(&package, "src/swr/index.ts").is_some(),
        "--swr should still emit its subtree alongside --refine"
    );
}

// --- RPC coverage: issue #571's follow-up lifts `RefineRequiresRest` to
// `RefineRequiresRestOrRpc`, so RPC schemas now get a manifest too. The
// four per-resource facts are transport-agnostic (`crate::refine`'s module
// doc); what has to change per transport is the `@cratestack/refine` type
// `cratestackRefineResources()` is typed to return.

#[test]
fn refine_supports_rpc_schemas_with_the_same_per_resource_facts_as_rest() {
    let package = generate(&rpc_schema(), true);
    let refine = file(&package, "src/refine.ts");

    assert!(
        refine.contains(
            "\"widgets\": {\n      api: client.widgets,\n      primaryKey: \"id\",\n      paged: false,\n    },"
        ),
        "widgets entry is wrong for an RPC schema:\n{refine}"
    );
    assert!(
        refine.contains(
            "\"ledgers\": {\n      api: client.ledgers,\n      primaryKey: \"id\",\n      paged: true,\n      versionField: \"revision\",\n    },"
        ),
        "ledgers entry is wrong for an RPC schema:\n{refine}"
    );
    assert!(
        refine.contains("primaryKey: \"sku\","),
        "products entry should carry the non-`id` primary key for an RPC schema too:\n{refine}"
    );
}

/// The contract with `@cratestack/refine`'s RPC provider (added alongside
/// this change): the emitted function is named identically across
/// transports (`cratestackRefineResources`) so consumer code doesn't
/// change when a schema switches transport — only the return *type*
/// switches, to `RpcResourceMap`.
#[test]
fn refine_types_an_rpc_schemas_manifest_as_rpc_resource_map() {
    let rpc_package = generate(&rpc_schema(), true);
    let rpc_refine = file(&rpc_package, "src/refine.ts");
    assert!(
        rpc_refine.contains("import type { RpcResourceMap } from \"@cratestack/refine\";"),
        "RPC refine.ts must import RpcResourceMap, not ResourceMap:\n{rpc_refine}"
    );
    assert!(
        rpc_refine.contains(
            "export function cratestackRefineResources(client: RefineTestClientClient): RpcResourceMap {"
        ),
        "RPC refine.ts's cratestackRefineResources() must return RpcResourceMap:\n{rpc_refine}"
    );
    assert!(
        !rpc_refine.contains("): ResourceMap {"),
        "RPC refine.ts must not fall back to the REST ResourceMap type:\n{rpc_refine}"
    );

    // …and REST keeps the plain `ResourceMap` it always had — the RPC
    // branch must be additive to the type name, not a global rename.
    let rest_package = generate(REFINE_SCHEMA, true);
    let rest_refine = file(&rest_package, "src/refine.ts");
    assert!(
        rest_refine.contains("import type { ResourceMap } from \"@cratestack/refine\";"),
        "REST refine.ts must still import the plain ResourceMap:\n{rest_refine}"
    );
    assert!(
        !rest_refine.contains("RpcResourceMap"),
        "REST refine.ts must not mention RpcResourceMap at all:\n{rest_refine}"
    );
}

#[test]
fn refine_flag_is_additive_for_rpc_schemas_too() {
    let source = rpc_schema();
    let plain = generate(&source, false);
    let with_refine = generate(&source, true);

    assert!(
        !plain.files.iter().any(|f| f.file_name == "src/refine.ts"),
        "src/refine.ts must not be emitted for an RPC schema without the flag"
    );

    for file in &plain.files {
        let counterpart = with_refine
            .files
            .iter()
            .find(|candidate| candidate.file_name == file.file_name)
            .unwrap_or_else(|| panic!("--refine dropped {} for an RPC schema", file.file_name));
        if matches!(file.file_name.as_str(), "package.json" | "src/index.ts") {
            continue;
        }
        assert_eq!(
            file.contents, counterpart.contents,
            "--refine changed {} for an RPC schema — it must only ADD a file",
            file.file_name
        );
    }
}

#[test]
fn refine_flag_re_exports_from_rpc_index_too() {
    let with_refine = generate(&rpc_schema(), true);
    assert!(
        file(&with_refine, "src/index.ts").contains("export * from \"./refine.js\";"),
        "RPC src/index.ts should re-export the generated manifest"
    );

    let without_refine = generate(&rpc_schema(), false);
    assert!(
        !file(&without_refine, "src/index.ts").contains("refine"),
        "RPC src/index.ts must not mention refine at all without the flag"
    );
}

fn rpc_schema() -> String {
    REFINE_SCHEMA.replace("datasource db {", "transport rpc\n\ndatasource db {")
}

fn generate(source: &str, refine: bool) -> GeneratedTypeScriptPackage {
    try_generate(source, refine, false).expect("fixture schema should generate")
}

fn try_generate(
    source: &str,
    refine: bool,
    swr: bool,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "refine-test-client".to_owned(),
            swr,
            refine,
            ..TypeScriptGeneratorConfig::default()
        },
    )
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
        .unwrap_or_else(|| panic!("generated package should contain {name}"))
}

fn file_named<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> Option<&'a str> {
    package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .map(|file| file.contents.as_str())
}
