//! Real npm/vitest proof of the generated TypeScript clients' opt-in
//! `compactKeys` option (compact wire keys): the negotiation headers, the
//! transcoding of a compact response, the adaptive compact request, and the
//! silent fallback to named when the `@cratestack/cbor` binding is absent or
//! predates `createCompactKeys`. `@cratestack/cbor` is mocked with a fake
//! `createCompactKeys` (`tests/js/compact_keys/fake-keys.ts`): the real
//! transcoders are proven by the codec crates and the bindings' own tests.
//!
//! Each run generates the default and the `--swr` layout together (they render
//! the same runtime templates) and type-checks the whole package with `tsc`
//! before vitest runs. Skips (printed) when `node`/`npm`/`npx` are not on
//! `PATH`, like every Node-driven test in this crate; CI runs it.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod vitest_support;
use vitest_support::vitest_command;

const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/compact_keys");

#[test]
fn rpc_runtime_negotiates_compact_keys_and_falls_back_to_named() {
    run(
        "rpc_runtime_negotiates_compact_keys_and_falls_back_to_named",
        "tests/fixtures/compact_rpc.cstack",
        &["compact-rpc.test.ts", "compact-rpc-missing.test.ts"],
    );
}

#[test]
fn rest_runtime_negotiates_compact_keys_and_falls_back_to_named() {
    run(
        "rest_runtime_negotiates_compact_keys_and_falls_back_to_named",
        "tests/fixtures/tiny_rest.cstack",
        &[
            "compact-rest.test.ts",
            "compact-rest-missing.test.ts",
            "compact-rest-absent.test.ts",
        ],
    );
}

fn run(test_name: &str, fixture: &str, test_files: &[&str]) {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping {test_name}: `node`/`npm`/`npx` not on PATH (expected only where Node is \
             absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file(fixture)
        .unwrap_or_else(|error| panic!("fixture {fixture:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "compact-keys-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("template should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
    for asset in ["package.json", "vitest.config.ts", "fake-keys.ts"]
        .iter()
        .chain(test_files.iter())
    {
        fs::copy(format!("{JS_DIR}/{asset}"), dir.path().join(asset))
            .unwrap_or_else(|error| panic!("copy {asset} into the generated package: {error}"));
    }

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

    let tsc = Command::new("npx")
        .args(["--yes", "tsc", "-p", "tsconfig.json", "--noEmit"])
        .current_dir(dir.path())
        .output()
        .expect("run tsc");
    assert!(
        tsc.status.success(),
        "tsc rejected the generated package ({test_name}):\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&tsc.stdout),
        String::from_utf8_lossy(&tsc.stderr)
    );

    let vitest = vitest_command(dir.path()).output().expect("run npx vitest");
    let stdout = String::from_utf8_lossy(&vitest.stdout);
    let stderr = String::from_utf8_lossy(&vitest.stderr);
    assert!(
        vitest.status.success(),
        "vitest run against the generated compact-keys client failed ({test_name}):\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    for file in test_files {
        assert!(
            stdout.contains(file),
            "vitest did not report running {file} ({test_name}):\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    let files = test_files.len();
    assert!(
        stdout.contains(&format!("Test Files  {files} passed ({files})"))
            && !stdout.contains("skipped")
            && !stdout.contains("todo"),
        "vitest did not pass all {files} file(s) cleanly ({test_name}):\nstdout: {stdout}"
    );
}

/// A REST package with no `@cratestack/cbor` installed still type-checks: the
/// optional peer is reached through a dynamic `import()` the compiler must not
/// insist on resolving.
#[test]
fn rest_package_typechecks_without_the_optional_cbor_peer() {
    if !node_npm_npx_available() {
        eprintln!("skipping rest_package_typechecks_without_the_optional_cbor_peer: no node");
        return;
    }
    let schema = cratestack_parser::parse_schema_file("tests/fixtures/tiny_rest.cstack")
        .expect("fixture should parse");
    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("template should render");
    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
    fs::write(
        dir.path().join("package.json"),
        r#"{"name":"no-cbor","version":"0.0.0","private":true,"type":"module",
"dependencies":{"decimal.js":"^10.6.0"},"devDependencies":{"typescript":"^7.0.2"}}"#,
    )
    .expect("write package.json");
    let install = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path())
        .output()
        .expect("run npm install");
    assert!(install.status.success(), "npm install failed");
    let tsc = Command::new("npx")
        .args(["--yes", "tsc", "-p", "tsconfig.json", "--noEmit"])
        .current_dir(dir.path())
        .output()
        .expect("run tsc");
    assert!(
        tsc.status.success(),
        "tsc failed without @cratestack/cbor:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&tsc.stdout),
        String::from_utf8_lossy(&tsc.stderr)
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
