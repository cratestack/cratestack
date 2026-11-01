//! Real npm/vitest proof of the generated TypeScript subscription surface
//! (slice S6, `docs/design/subscriptions-websocket.md` §8): generate a
//! package from `tests/fixtures/subscriptions.cstack` (a model feed and a
//! subscription procedure), copy `tests/js/subscriptions/*` beside it,
//! `npm install`, `npx vitest run`. The test file stands in for the server
//! with an SSE `fetch` and a fake `WebSocket`; the Rust server side of the
//! same wire is proven by `cratestack-pg/tests/websocket_subscriptions.rs`.
//!
//! Skips (printed) when `node`/`npm`/`npx` are not on `PATH`, like
//! `bytes_round_trip.rs`; CI runs it.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

/// Keep in lockstep with `tests/js/subscriptions/subscriptions.test.ts` — a
/// successful vitest exit alone cannot tell a genuinely green run from an
/// accidentally-empty one (the same guard `decimal_round_trip.rs` uses).
const EXPECTED_PASSING: &str = "4 passed";

#[test]
fn subscriptions_run_over_sse_and_a_socket() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping subscriptions_run_over_sse_and_a_socket: \
             `node`/`npm`/`npx` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this — \
             see tests/subscriptions_runtime.rs's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file("tests/fixtures/subscriptions.cstack")
        .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            native_cbor: false,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("default template should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Overwrite the generated package.json with this test's fixture one,
    // which adds the `vitest`/`typescript` devDependencies needed to
    // actually run the test below — same reason `decimal_round_trip.rs`
    // does this for its own fixture.
    let js_fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/subscriptions");
    for asset in ["package.json", "vitest.config.ts", "subscriptions.test.ts"] {
        fs::copy(format!("{js_fixture_dir}/{asset}"), dir.path().join(asset))
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

    let test_run = Command::new("npx")
        .args(["--yes", "vitest", "run"])
        .current_dir(dir.path())
        .output()
        .expect("run npx vitest");

    let stdout = String::from_utf8_lossy(&test_run.stdout);
    let stderr = String::from_utf8_lossy(&test_run.stderr);
    assert!(
        test_run.status.success(),
        "vitest run against the generated subscription client failed:\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(EXPECTED_PASSING) || stderr.contains(EXPECTED_PASSING),
        "expected vitest to report exactly {EXPECTED_PASSING} tests (the subscription suite):\nstdout: {stdout}\nstderr: {stderr}"
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
