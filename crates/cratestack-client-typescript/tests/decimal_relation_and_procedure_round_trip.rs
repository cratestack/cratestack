//! Real `npm install` + `npx vitest run` proof for cratestack#499's
//! remediation of #498's F2 (procedure return type revival, including a
//! bare scalar `Decimal` return), F3 (the `swr` preset's decode-side
//! revival), and F5 (relation-embedded field revival) — not generated-text
//! assertions. Generates `tests/fixtures/decimal_relation_and_procedure.cstack`
//! with both the `default` and `swr` presets and runs a real vitest suite
//! against each, mirroring `tests/decimal_round_trip.rs`'s pattern.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't
//! on `PATH` — same rationale as `tests/decimal_round_trip.rs`.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const FIXTURE: &str = "tests/fixtures/decimal_relation_and_procedure.cstack";
const JS_FIXTURE_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/js/decimal_relation_and_procedure"
);

#[test]
fn default_layout_revives_relation_and_procedure_decimal_fields() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping default_layout_revives_relation_and_procedure_decimal_fields: \
             `node`/`npm`/`npx` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file(FIXTURE).expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "decimal-relation-and-procedure-check".to_owned(),
            swr: false,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("default template should render");

    run_vitest_against_generated_package(&package, "default.test.ts", 3);
}

#[test]
fn swr_layout_revives_relation_and_procedure_decimal_fields() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping swr_layout_revives_relation_and_procedure_decimal_fields: \
             `node`/`npm`/`npx` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file(FIXTURE).expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "decimal-relation-and-procedure-check-swr".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("--swr template should render");

    run_vitest_against_generated_package(&package, "swr.test.ts", 3);
}

fn run_vitest_against_generated_package(
    package: &cratestack_client_typescript::GeneratedTypeScriptPackage,
    test_file: &str,
    expected_passed: u32,
) {
    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    for asset in ["package.json", "vitest.config.ts"] {
        fs::copy(format!("{JS_FIXTURE_DIR}/{asset}"), dir.path().join(asset))
            .unwrap_or_else(|error| panic!("copy {asset} into generated package dir: {error}"));
    }
    fs::copy(
        format!("{JS_FIXTURE_DIR}/{test_file}"),
        dir.path().join("decimal.test.ts"),
    )
    .unwrap_or_else(|error| panic!("copy {test_file} into generated package dir: {error}"));

    let install = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path())
        .output()
        .expect("run npm install");
    assert!(
        install.status.success(),
        "npm install failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    let test_run = Command::new("npx")
        .args(["--yes", "vitest", "run"])
        .current_dir(dir.path())
        .output()
        .expect("run npx vitest");

    let stdout = String::from_utf8_lossy(&test_run.stdout);
    let stderr = String::from_utf8_lossy(&test_run.stderr);
    assert!(
        test_run.status.success(),
        "vitest run against the generated Decimal client ({test_file}) failed — this is the \
         real revival proof for cratestack#499, not a Rust string assertion:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    let marker = format!("{expected_passed} passed");
    assert!(
        stdout.contains(&marker) || stderr.contains(&marker),
        "expected vitest to report exactly {expected_passed} passed tests ({test_file}):\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
