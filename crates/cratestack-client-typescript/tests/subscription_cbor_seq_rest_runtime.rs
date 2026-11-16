//! Real npm/vitest proof that a generated *REST* client revives a cbor-seq
//! model event (cratestack#1001): generate a package from
//! `tests/fixtures/subscription_cbor_seq_rest.cstack`, copy
//! `tests/js/subscriptions-rest/*` and the shared real-server body
//! `tests/js/subscriptions/cbor-seq-model-feed.hex` beside it, `npm install`,
//! `npx vitest run`. The RPC client's subscription suite is
//! `subscriptions_runtime.rs`.
//!
//! Skips (printed) when `node`/`npm`/`npx` are not on `PATH`; CI runs it.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

/// Keep in lockstep with `tests/js/subscriptions-rest/*.test.ts` (1 + 3).
const EXPECTED_PASSING: &str = "4 passed";

#[test]
fn a_rest_client_revives_a_cbor_seq_model_event() {
    if !["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }) {
        eprintln!(
            "skipping a_rest_client_revives_a_cbor_seq_model_event: `node`/`npm`/`npx` not on PATH"
        );
        return;
    }
    let schema =
        cratestack_parser::parse_schema_file("tests/fixtures/subscription_cbor_seq_rest.cstack")
            .expect("fixture should parse");
    let package = generate_package(&schema, &TypeScriptGeneratorConfig::default())
        .expect("default template should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }
    let js = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js");
    for asset in [
        "subscriptions-rest/package.json",
        "subscriptions-rest/vitest.config.ts",
        "subscriptions-rest/cbor-seq-rest.test.ts",
        "subscriptions-rest/cbor-seq-codec.test.ts",
        "subscriptions/cbor-seq-model-feed.hex",
    ] {
        let name = asset.rsplit('/').next().expect("a file name");
        fs::copy(format!("{js}/{asset}"), dir.path().join(name))
            .unwrap_or_else(|error| panic!("copy {asset} into generated package dir: {error}"));
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
    let run = Command::new("npx")
        .args(["--yes", "vitest", "run"])
        .current_dir(dir.path())
        .output()
        .expect("run npx vitest");
    let stdout = String::from_utf8_lossy(&run.stdout);
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        run.status.success(),
        "vitest run against the generated REST client failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(EXPECTED_PASSING) || stderr.contains(EXPECTED_PASSING),
        "expected vitest to report exactly {EXPECTED_PASSING}:\nstdout: {stdout}\nstderr: {stderr}"
    );
}
