//! Real npm/vitest proof that the generated TypeScript client's `BigInt`
//! support (ADR 0019) behaves as documented, not just that the generated
//! *text* looks right. Modelled on `tests/decimal_round_trip.rs`: generate a
//! real package from a fixture, copy this crate's `tests/js/bigint_round_trip/*`
//! assets alongside it, `npm install`, `npx vitest run`.
//!
//! What each run proves is in the vitest file named next to it:
//!
//! | run | fixture | vitest file |
//! |---|---|---|
//! | REST, default + `--swr` layouts | `bigint_scalar.cstack` | `bigint.test.ts`, `bigint-swr.test.ts` |
//! | RPC on the pure-TS codec, plus `@cratestack/link-batch` | `bigint_scalar_rpc.cstack` | `bigint-rpc.test.ts` |
//! | REST `computedParams` | `bigint_computed_params.cstack` | `bigint-computed-rest.test.ts` |
//! | RPC `computedParams` | `bigint_computed_params_rpc.cstack` | `bigint-computed-rpc.test.ts` |
//!
//! The real `@cratestack/cbor` codec is `tests/native_cbor_bigint_encode.rs`.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't on
//! `PATH`, the same convention as every Node-driven test in this crate; CI
//! runs it because `ubuntu-latest` ships Node.

use std::fs;
use std::path::Path;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/bigint_round_trip");
const LINK_BATCH_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/cratestack-link-batch/src"
);

#[test]
fn rest_client_holds_bigint_exactly_in_both_layouts() {
    run(
        "rest_client_holds_bigint_exactly_in_both_layouts",
        "tests/fixtures/bigint_scalar.cstack",
        TypeScriptGeneratorConfig {
            package_name: "bigint-round-trip-check".to_owned(),
            swr: true,
            ..TypeScriptGeneratorConfig::default()
        },
        &["bigint.test.ts", "bigint-swr.test.ts"],
        false,
    );
}

#[test]
fn rpc_client_holds_bigint_exactly_and_batches_it_as_a_string() {
    run(
        "rpc_client_holds_bigint_exactly_and_batches_it_as_a_string",
        "tests/fixtures/bigint_scalar_rpc.cstack",
        TypeScriptGeneratorConfig {
            package_name: "bigint-round-trip-rpc-check".to_owned(),
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
        &["bigint-rpc.test.ts"],
        true,
    );
}

#[test]
fn rest_computed_params_convert_a_bigint_before_json_stringify() {
    run(
        "rest_computed_params_convert_a_bigint_before_json_stringify",
        "tests/fixtures/bigint_computed_params.cstack",
        TypeScriptGeneratorConfig {
            package_name: "bigint-computed-rest-check".to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
        &["bigint-computed-rest.test.ts"],
        false,
    );
}

#[test]
fn rpc_computed_params_convert_a_bigint_before_json_stringify() {
    run(
        "rpc_computed_params_convert_a_bigint_before_json_stringify",
        "tests/fixtures/bigint_computed_params_rpc.cstack",
        TypeScriptGeneratorConfig {
            package_name: "bigint-computed-rpc-check".to_owned(),
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
        &["bigint-computed-rpc.test.ts"],
        false,
    );
}

fn run(
    test_name: &str,
    fixture: &str,
    config: TypeScriptGeneratorConfig,
    test_files: &[&str],
    with_link_batch: bool,
) {
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
    if with_link_batch {
        copy_dir(Path::new(LINK_BATCH_SRC), &dir.path().join("link-batch"));
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

    let vitest = Command::new("npx")
        .args(["--yes", "vitest", "run", "--reporter=verbose"])
        .current_dir(dir.path())
        .output()
        .expect("run npx vitest");
    let stdout = String::from_utf8_lossy(&vitest.stdout);
    let stderr = String::from_utf8_lossy(&vitest.stderr);
    assert!(
        vitest.status.success(),
        "vitest run against the generated BigInt client failed ({test_name}):\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // A successful exit alone cannot tell a green run from an accidentally
    // empty one (same belt-and-braces as `decimal_round_trip.rs`): the
    // verbose reporter names every file it ran, and a skipped or todo test
    // would show up in the summary.
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

/// Copies the `.ts` files of `from` (no subdirectories needed) into `to`.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create link-batch dir");
    for entry in fs::read_dir(from).expect("read link-batch src") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "ts") {
            fs::copy(&path, to.join(path.file_name().expect("file name")))
                .expect("copy link-batch");
        }
    }
}

fn node_npm_npx_available() -> bool {
    ["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
