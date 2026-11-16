//! Real npm/vitest proof of the generated TypeScript client's signed
//! subscription open (cratestack#1378): generate a package from
//! `tests/fixtures/subscription_frames_rpc.cstack` (a marked and an unmarked
//! subscription procedure), copy `tests/js/subscriptions-sealed/*` beside it,
//! `npm install`, `npx vitest run`. The sealer is a fake there; the real one,
//! `createSealedSubscriptions`, is tested in `packages/cratestack-cbor`, and
//! the generated text for both transports in `sealed_subscriptions_generator.rs`.
//!
//! Skips (printed) when `node`/`npm`/`npx` are not on `PATH`; CI runs it.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

/// Keep in lockstep with `tests/js/subscriptions-sealed/sealed.test.ts` (5 + 2 + 1).
const EXPECTED_PASSING: &str = "8 passed";

#[test]
fn a_generated_client_opens_a_marked_subscription_through_its_sealer() {
    if !["node", "npm", "npx"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }) {
        eprintln!(
            "skipping a_generated_client_opens_a_marked_subscription_through_its_sealer: \
             `node`/`npm`/`npx` not on PATH"
        );
        return;
    }
    let schema =
        cratestack_parser::parse_schema_file("tests/fixtures/subscription_frames_rpc.cstack")
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
    let js = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/subscriptions-sealed");
    for asset in ["package.json", "vitest.config.ts", "sealed.test.ts"] {
        fs::copy(format!("{js}/{asset}"), dir.path().join(asset))
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
        "vitest run against the generated sealed-subscription client failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(EXPECTED_PASSING) || stderr.contains(EXPECTED_PASSING),
        "expected vitest to report exactly {EXPECTED_PASSING}:\nstdout: {stdout}\nstderr: {stderr}"
    );
}
