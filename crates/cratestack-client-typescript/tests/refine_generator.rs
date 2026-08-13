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
    TypeScriptPreset, generate_package,
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
fn refine_is_rejected_for_transports_whose_client_shape_it_cannot_drive() {
    // `@cratestack/refine` builds a `CratestackFetchQuery` into
    // `list(options)`. The RPC client takes its query positionally, as a
    // different type — an emitted refine.ts would fail `tsc` in the
    // consumer's package, so the generator refuses instead.
    let rpc_schema = REFINE_SCHEMA.replace("datasource db {", "transport rpc\n\ndatasource db {");
    let error = try_generate(&rpc_schema, true, TypeScriptPreset::Default)
        .expect_err("--refine on an RPC schema should be rejected");
    assert!(
        matches!(error, TypeScriptGeneratorError::RefineRequiresRest),
        "expected RefineRequiresRest, got: {error}"
    );
    // …and the same schema without the flag still generates fine, so the
    // rejection is scoped to the flag and not to the schema.
    try_generate(&rpc_schema, false, TypeScriptPreset::Default)
        .expect("an RPC schema without --refine is unaffected");
}

#[test]
fn refine_is_rejected_for_the_swr_preset_which_has_no_client_class() {
    let error = try_generate(REFINE_SCHEMA, true, TypeScriptPreset::Swr)
        .expect_err("--refine with --preset swr should be rejected");
    assert!(
        matches!(error, TypeScriptGeneratorError::RefineUnsupportedPreset),
        "expected RefineUnsupportedPreset, got: {error}"
    );
}

fn generate(source: &str, refine: bool) -> GeneratedTypeScriptPackage {
    try_generate(source, refine, TypeScriptPreset::Default).expect("fixture schema should generate")
}

fn try_generate(
    source: &str,
    refine: bool,
    preset: TypeScriptPreset,
) -> Result<GeneratedTypeScriptPackage, TypeScriptGeneratorError> {
    let schema = cratestack_parser::parse_schema(source).expect("fixture schema should parse");
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "refine-test-client".to_owned(),
            preset,
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
