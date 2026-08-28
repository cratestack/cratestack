//! Real `npm install` + `npx vitest run` proof for cratestack#499's review
//! remediation of F5: an earlier version of `crate::decimal` kept a flat,
//! schema-wide-unioned `Set<string>` of `Decimal` field *names* reachable
//! from a response's root type and matched it against a decoded response's
//! keys at *any* nesting depth. That's unsound the moment two *different*
//! reachable types can each contribute a field name to the same flat set:
//! a non-`Decimal` field in one type sharing a name with a `Decimal` field
//! in another reachable type gets wrongly converted.
//!
//! `tests/fixtures/decimal_name_collision.cstack` is the minimal
//! reproduction: `Order.total: Decimal`, related `Account.total: String`.
//! This test generates a real client from it and runs a real vitest suite
//! (`tests/js/decimal_collision_regression/collision.test.ts`) proving
//! `Account.total` survives untouched (both a numeric-looking value, which
//! the old scheme silently corrupted, and a non-numeric one, which the old
//! scheme threw decoding) while `Order.total` still revives correctly —
//! not a generated-text assertion.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx` aren't
//! on `PATH` — same rationale as `tests/decimal_round_trip.rs`.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn related_models_with_a_same_named_non_decimal_field_do_not_collide() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping related_models_with_a_same_named_non_decimal_field_do_not_collide: \
             `node`/`npm`/`npx` not on PATH (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this)"
        );
        return;
    }

    let schema =
        cratestack_parser::parse_schema_file("tests/fixtures/decimal_name_collision.cstack")
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

    let js_fixture_dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/js/decimal_collision_regression"
    );
    for asset in ["package.json", "vitest.config.ts", "collision.test.ts"] {
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
        "vitest run against the generated Order/Account client failed — this is the real \
         collision-fix proof for cratestack#499, not a Rust string assertion:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("4 passed") || stderr.contains("4 passed"),
        "expected vitest to report exactly 4 passed tests:\nstdout: {stdout}\nstderr: {stderr}"
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
