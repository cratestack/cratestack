use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

#[test]
fn swr_rpc_file_set_is_additive_to_the_default_layout() {
    let package = generate_for("tiny_rpc", true);
    let mut names: Vec<&str> = package
        .files
        .iter()
        .map(|file| file.file_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(
        names,
        vec![
            "README.md",
            "package.json",
            "src/cbor-argument.ts",
            "src/cbor-item.ts",
            "src/cbor-seq.ts",
            "src/client.ts",
            "src/compact.ts",
            "src/index.ts",
            "src/links.ts",
            "src/models.ts",
            "src/queries.ts",
            "src/runtime.ts",
            "src/stream-terminal.ts",
            "src/swr/cbor-argument.ts",
            "src/swr/cbor-item.ts",
            "src/swr/cbor-seq.ts",
            "src/swr/compact.ts",
            "src/swr/index.ts",
            "src/swr/links.ts",
            "src/swr/models/shared.ts",
            "src/swr/models/widget.hooks.ts",
            "src/swr/models/widget.ts",
            "src/swr/procedures.hooks.ts",
            "src/swr/procedures.ts",
            "src/swr/queries.ts",
            "src/swr/runtime.ts",
            "src/swr/stream-terminal.ts",
            "src/swr/swr-keys.ts",
            "tsconfig.json",
        ],
        "--swr's RPC file set changed unexpectedly (the default layout must \
         stay present alongside src/swr/**; src/react-query.ts is absent here \
         because --tanstack, issue #617, is off in this fixture)"
    );
}

fn generate_for(fixture_stem: &str, swr: bool) -> GeneratedTypeScriptPackage {
    let fixture_path = format!("tests/fixtures/{fixture_stem}.cstack");
    let schema = cratestack_parser::parse_schema_file(&fixture_path)
        .unwrap_or_else(|error| panic!("fixture {fixture_path:?} should parse: {error}"));
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-fixture-client".to_owned(),
            swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("generation should succeed for this fixture/flag combination")
}
