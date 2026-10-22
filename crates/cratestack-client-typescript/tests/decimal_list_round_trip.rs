//! Real npm/vitest proof that the generated TypeScript client revives a
//! `Decimal[]` field element by element (cratestack#1155), on a model and on
//! a `type` reached through a procedure, on both transports and both layouts.
//! Modelled on `tests/bigint_round_trip.rs`: generate a real package from a
//! fixture, copy this crate's `tests/js/decimal_list_round_trip/*` assets
//! alongside it, `npm install`, `npx vitest run`.
//!
//! | run | fixture | vitest files |
//! |---|---|---|
//! | REST, default + `--swr` layouts | `decimal_list.cstack` | `decimal-list.test.ts`, `decimal-list-swr.test.ts` |
//! | RPC on the pure-TS codec, default + `--swr` layouts | `decimal_list_rpc.cstack` | `decimal-list-rpc.test.ts`, `decimal-list-rpc-swr.test.ts` |
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't on
//! `PATH`, the same convention as every Node-driven test in this crate; CI
//! runs it because `ubuntu-latest` ships Node.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod vitest_support;
use vitest_support::vitest_command;

const JS_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/js/decimal_list_round_trip"
);

#[test]
fn rest_client_revives_decimal_list_elements_in_both_layouts() {
    run(
        "rest_client_revives_decimal_list_elements_in_both_layouts",
        "tests/fixtures/decimal_list.cstack",
        TypeScriptGeneratorConfig {
            package_name: "decimal-list-round-trip-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
        &["decimal-list.test.ts", "decimal-list-swr.test.ts"],
    );
}

#[test]
fn rpc_client_revives_decimal_list_elements_in_both_layouts() {
    run(
        "rpc_client_revives_decimal_list_elements_in_both_layouts",
        "tests/fixtures/decimal_list_rpc.cstack",
        TypeScriptGeneratorConfig {
            package_name: "decimal-list-round-trip-rpc-check".to_owned(),
            swr: true,
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
        &["decimal-list-rpc.test.ts", "decimal-list-rpc-swr.test.ts"],
    );
}

fn run(test_name: &str, fixture: &str, config: TypeScriptGeneratorConfig, test_files: &[&str]) {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping {test_name}: `node`/`npm`/`npx` not on PATH (expected only where Node is \
             absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file(fixture)
        .unwrap_or_else(|error| panic!("fixture {fixture:?} should parse: {error}"));
    let package = generate_package(&schema, &config).expect("template should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
    // Overwrite the generated package.json with this suite's, which adds the
    // `vitest`/`typescript` devDependencies (same reason as
    // `decimal_round_trip.rs`).
    for asset in ["package.json", "vitest.config.ts"]
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

    let vitest = vitest_command(dir.path()).output().expect("run npx vitest");
    let stdout = String::from_utf8_lossy(&vitest.stdout);
    let stderr = String::from_utf8_lossy(&vitest.stderr);
    assert!(
        vitest.status.success(),
        "vitest run against the generated Decimal[] client failed ({test_name}):\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // A successful exit alone cannot tell a green run from an accidentally
    // empty one: the verbose reporter names every file it ran, and a skipped
    // or todo test would show up in the summary.
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

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
