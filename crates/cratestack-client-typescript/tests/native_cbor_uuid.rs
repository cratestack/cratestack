//! Real, Node-driven proof that a generated RPC client revives the 16-byte
//! `Uuid` a server writes under CBOR into the canonical string, under the
//! actually-published `@cratestack/cbor` (the default RPC codec), and keeps
//! sending `Uuid`s as CBOR *text* — which the server accepts
//! (cratestack#1359; the server half is `cratestack-api`'s
//! `cbor_uuid_text_rpc.rs`). Modelled on `native_cbor_bigint_encode.rs`.
//!
//! ## Why the real codec, not a stub
//!
//! The bug is that the real codec decodes a CBOR byte string to a
//! `Uint8Array`. A stub built on `JSON.stringify` has no byte-string arm, so
//! it could never show it. The scripts build their responses with the real
//! codec encoding a `Uint8Array` (a CBOR byte string, major type 2: what the
//! server writes) and decode the captured requests with it.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm` aren't on `PATH`.

use std::path::Path;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

mod support;
use support::{command_report, node_toolchain_available, tsx_command};

const FIXTURE: &str = "tests/fixtures/uuid_native_cbor.cstack";
const JS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/uuid_native_cbor");
const LINK_BATCH_SRC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../packages/cratestack-link-batch/src"
);

/// Every `Uuid` position decoded from a byte string, and every request
/// position still sent as text.
#[test]
fn byte_string_uuids_decode_to_strings_under_the_real_native_codec() {
    run_script(
        "byte_string_uuids_decode_to_strings_under_the_real_native_codec",
        "unary.mts",
        "NATIVE_CBOR_UUID_UNARY_OK",
        false,
        false,
    );
}

/// `runtime.batch()` and `@cratestack/link-batch`.
#[test]
fn batch_and_link_batch_revive_and_send_uuids_under_the_real_native_codec() {
    run_script(
        "batch_and_link_batch_revive_and_send_uuids_under_the_real_native_codec",
        "batch.mts",
        "NATIVE_CBOR_UUID_BATCH_OK",
        true,
        false,
    );
}

/// The `--swr` layout's own copy of the revival runtime.
#[test]
fn the_swr_layout_revives_uuids_too() {
    run_script(
        "the_swr_layout_revives_uuids_too",
        "swr.mts",
        "NATIVE_CBOR_UUID_SWR_OK",
        false,
        true,
    );
}

fn run_script(test_name: &str, script: &str, marker: &str, with_link_batch: bool, swr: bool) {
    if !node_toolchain_available() {
        eprintln!(
            "skipping {test_name}: `node`/`npm` not on PATH (expected only where Node is absent, \
             e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let dir = generate_and_write_package(swr);
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

fn generate_and_write_package(swr: bool) -> tempfile::TempDir {
    let schema = cratestack_parser::parse_schema_file(FIXTURE)
        .unwrap_or_else(|error| panic!("fixture {FIXTURE:?} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "uuid-native-cbor-check".to_owned(),
            swr,
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
