//! Real, Node-driven proof that a generated RPC client puts a `BigInt` on the
//! wire as a CBOR *text string* under the actually-published
//! `@cratestack/cbor` (the default RPC codec), and reads one back as an exact
//! `bigint`. Modelled on `tests/native_cbor_decimal_encode.rs`.
//!
//! ## Why the real codec, not a stub
//!
//! `@cratestack/cbor` accepts a JS `bigint` without complaint and writes a
//! CBOR **integer** for it (RAN: `9223372036854775807n` encodes as
//! `1b7fffffffffffffff`). A server refuses that for a `BigInt` field, so a
//! missed conversion is not an exception in the client but a request that is
//! silently the wrong type. A stub codec built on `JSON.stringify` would
//! throw instead, and so could never show that. This file `npm install`s the
//! real addon and decodes the captured wire bytes with it.
//!
//! ## What is pinned, and what is not
//!
//! The scripts assert the request bytes equal the real codec's encoding of the
//! plain-string request, and contain the text string (`0x73` plus 19 digits for
//! `i64::MAX`) byte for byte. The cross-language hex fixtures for the three
//! boundary values live with the codec bridges (B10) and are not repeated here.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm` aren't on `PATH`.

use std::path::Path;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const FIXTURE: &str = "tests/fixtures/bigint_native_cbor_encode.cstack";
const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/bigint_native_cbor");
const LINK_BATCH_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/cratestack-link-batch/src"
);

/// Decode, re-encode and unary encode of every `BigInt` position (field,
/// optional, list, `@id`, procedure argument and return), at the three
/// boundary values.
#[test]
fn unary_paths_use_cbor_text_strings_under_the_real_native_codec() {
    run_script(
        "unary_paths_use_cbor_text_strings_under_the_real_native_codec",
        "unary.mts",
        "NATIVE_CBOR_BIGINT_UNARY_OK",
        false,
    );
}

/// A CBOR integer at a `BigInt` key, which is what a pre-cutover server
/// sends, is refused rather than read as a number.
#[test]
fn a_cbor_integer_at_a_bigint_key_is_refused_under_the_real_native_codec() {
    run_script(
        "a_cbor_integer_at_a_bigint_key_is_refused_under_the_real_native_codec",
        "decode-refusal.mts",
        "NATIVE_CBOR_BIGINT_DECODE_REFUSAL_OK",
        false,
    );
}

/// `runtime.batch()` and `@cratestack/link-batch`, the latter of which
/// encodes raw inputs with the request's codec and never runs `terminalLink`.
#[test]
fn batch_and_link_batch_use_cbor_text_strings_under_the_real_native_codec() {
    run_script(
        "batch_and_link_batch_use_cbor_text_strings_under_the_real_native_codec",
        "batch.mts",
        "NATIVE_CBOR_BIGINT_BATCH_OK",
        true,
    );
}

fn run_script(test_name: &str, script: &str, marker: &str, with_link_batch: bool) {
    if !node_toolchain_available() {
        eprintln!(
            "skipping {test_name}: `node`/`npm` not on PATH (expected only where Node is absent, \
             e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let dir = generate_and_write_package();
    for asset in ["common.mts", script] {
        std::fs::copy(format!("{JS_DIR}/{asset}"), dir.path().join(asset))
            .unwrap_or_else(|error| panic!("copy {asset} into the generated package: {error}"));
    }
    if with_link_batch {
        copy_ts_files(Path::new(LINK_BATCH_SRC), &dir.path().join("link-batch"));
    }

    let mut install = Command::new("npm");
    install
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path());
    let installed = install.output().expect("run npm install");
    assert!(
        installed.status.success(),
        "npm install failed (this installs the REAL @cratestack/cbor from the registry, not a \
         stub):\n{}",
        command_report(&install, &installed)
    );

    let mut tsx = tsx_command(dir.path(), script);
    let output = tsx.output().expect("run tsx");
    assert!(
        output.status.success(),
        "{script} failed against the generated RPC client on the real @cratestack/cbor:\n{}",
        command_report(&tsx, &output)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(marker),
        "{script} did not print its success marker {marker}:\n{}",
        command_report(&tsx, &output)
    );
}

fn generate_and_write_package() -> tempfile::TempDir {
    let schema = cratestack_parser::parse_schema_file(FIXTURE)
        .unwrap_or_else(|error| panic!("fixture {FIXTURE:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "bigint-native-cbor-encode-check".to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("default template should render: {error}"));

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }
    dir
}

/// Copies the `.ts` files of `from` into `to` (link-batch's `src/` is flat).
fn copy_ts_files(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create link-batch dir");
    for entry in std::fs::read_dir(from).expect("read link-batch src") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "ts") {
            std::fs::copy(&path, to.join(path.file_name().expect("file name")))
                .expect("copy link-batch source");
        }
    }
}
