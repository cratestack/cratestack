//! Ordinal enums (`Pending = 1`, `docs/design/enum-ordinals.md`) in the
//! TypeScript client. A response carries the number; the generated client
//! revives it to the variant's name, so the app-facing type stays the
//! string union. The fixture's header lists every place an ordinal sits.
//!
//! The text locks pin the registry and that a schema without an ordinal
//! enum renders none of it (the snapshots prove that byte for byte). The
//! Node-driven test runs `tests/js/ordinal_enums/smoke.ts` against the
//! generated package, both layouts, through the pinned `tsx` (skips,
//! printed, when `node`/`npm` aren't on `PATH`; CI runs it).

use cratestack_client_typescript::{
    GeneratedTypeScriptPackage, TypeScriptGeneratorConfig, generate_package,
};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ordinal_enums.cstack"
);
const SMOKE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/js/ordinal_enums/smoke.ts"
);

fn generate(source: &str) -> GeneratedTypeScriptPackage {
    let schema = cratestack_parser::parse_schema(source).expect("fixture validates");
    generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "ordinal-enums-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("template should render")
}

fn fixture() -> String {
    std::fs::read_to_string(FIXTURE).expect("read the fixture")
}

fn file<'a>(package: &'a GeneratedTypeScriptPackage, name: &str) -> &'a str {
    &package
        .files
        .iter()
        .find(|file| file.file_name == name)
        .unwrap_or_else(|| panic!("generated package has no {name}"))
        .contents
}

#[test]
fn the_registry_maps_numbers_to_names_on_both_transports_and_layouts() {
    let rest = generate(&fixture());
    let rpc = generate(&format!("transport rpc\n\n{}", fixture()));
    for package in [&rest, &rpc] {
        for models in [
            file(package, "src/models.ts"),
            file(package, "src/swr/models/shared.ts"),
        ] {
            for expected in [
                "  PaymentStatus: { 1: 'Pending', 2: 'Approved', 5: 'Refunded' },",
                "  Payment: { 'status': 'PaymentStatus', 'previous': 'PaymentStatus' },",
                "  Summary: { 'current': 'PaymentStatus', 'history': 'PaymentStatus' },",
                // The app-facing type is still the names.
                "export type PaymentStatus = 'Pending' | 'Approved' | 'Refunded';",
            ] {
                assert!(models.contains(expected), "missing `{expected}`:\n{models}");
            }
            // A string enum is no ordinal, and `Refund.status` is a String.
            assert!(!models.contains("  Channel: { "), "{models}");
            assert!(!models.contains("  Refund: { 'status'"), "{models}");
        }
    }
    assert!(
        file(&rest, "src/client.ts")
            .contains("reviveWireFields(value, 'PaymentStatus') as PaymentStatus"),
        "a bare ordinal enum return must be revived"
    );
}

#[test]
fn a_schema_without_an_ordinal_enum_renders_no_registry() {
    let package =
        generate("enum Channel {\n  card\n}\n\ntype T {\n  c Channel\n}\n\nprocedure t(): T\n");
    for name in ["src/models.ts", "src/swr/models/shared.ts"] {
        let models = file(&package, name);
        assert!(!models.contains("ordinalEnum"), "{name}:\n{models}");
        assert!(!models.contains("toEnumName"), "{name}:\n{models}");
    }
}

#[test]
fn generated_revival_turns_ordinals_into_names() {
    if !node_toolchain_available() {
        eprintln!(
            "skipping generated_revival_turns_ordinals_into_names: `node`/`npm` not on PATH \
             (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }
    let package = generate(&fixture());
    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }
    std::fs::copy(SMOKE, dir.path().join("smoke.ts")).expect("copy the smoke script");

    let mut install = std::process::Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed:\n{}",
        command_report(&install, &installed)
    );

    let mut tsx = tsx_command(dir.path(), "smoke.ts");
    let output = tsx.output().expect("run tsx");
    assert!(
        output.status.success()
            && String::from_utf8_lossy(&output.stdout).contains("ORDINAL_REVIVAL_OK"),
        "smoke script failed:\n{}",
        command_report(&tsx, &output)
    );
}
