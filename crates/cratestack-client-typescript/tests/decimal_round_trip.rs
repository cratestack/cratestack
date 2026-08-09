//! Real npm/vitest proof that the generated TypeScript client's `Decimal`
//! support (cratestack#498) actually behaves as documented, not just that
//! the generated *text* looks right (`tests/generator.rs`'s
//! `decimal_scalar_maps_to_a_real_declared_decimal_type`/
//! `decimal_scalar_revives_on_decode_over_rpc_transport_too` prove that
//! half). Mirrors `tests/swr_hooks_invalidation.rs`'s pattern exactly:
//! generate a real package from `tests/fixtures/decimal_scalar.cstack`,
//! copy this crate's own `tests/js/decimal_round_trip/*` test assets
//! alongside it, `npm install`, `npx vitest run`.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't
//! on `PATH` — same rationale as `swr_hooks_invalidation.rs`: no Rust CI
//! job in this repo currently provisions Node (the Node-provisioned job,
//! `typescript-verify`, only runs `tsc`, not this crate's own `cargo
//! test`).

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn decimal_round_trips_through_the_generated_rest_client() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping decimal_round_trips_through_the_generated_rest_client: \
             `node`/`npm`/`npx` not on PATH (expected in this repo's Rust-only CI jobs — \
             see tests/decimal_round_trip.rs's module doc)"
        );
        return;
    }

    let schema = cratestack_parser::parse_schema_file("tests/fixtures/decimal_scalar.cstack")
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

    // Overwrite the generated package.json with this test's fixture one,
    // which adds the `vitest`/`typescript` devDependencies needed to
    // actually run the test below — same reason
    // `swr_hooks_invalidation.rs` does this for its own fixture.
    let js_fixture_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/js/decimal_round_trip");
    for asset in ["package.json", "vitest.config.ts", "decimal.test.ts"] {
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
        "vitest run against the generated Decimal client failed — this is the real \
         round-trip proof for cratestack#498 requirements 1-3, not a Rust string \
         assertion:\nstdout: {stdout}\nstderr: {stderr}"
    );
    // Belt-and-suspenders (matching `swr_hooks_invalidation.rs`): a
    // successful exit alone can't distinguish a genuinely green run from
    // an accidentally-empty one.
    assert!(
        stdout.contains("7 passed") || stderr.contains("7 passed"),
        "expected vitest to report exactly 7 passed tests (the round-trip proof suite):\n\
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
