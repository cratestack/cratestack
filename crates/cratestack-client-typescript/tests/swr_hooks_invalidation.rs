//! Real vitest + @testing-library/react proof for issue #305 AC #6
//! ("Invalidation is proven by test, not asserted: a test renders a
//! hook, performs a mutation, and confirms the dependent query
//! refetches AND that an unrelated query does not") and AC #7 (SWR's
//! null-key conditional-fetching idiom for a "no argument yet" read
//! hook). `tests/swr_generator.rs` proves the invalidation *code* is
//! emitted (the right `mutate(...)` calls appear in the generated
//! text); this proves the emitted code actually behaves that way when
//! it runs, the same "generated text vs. generated behavior" gap
//! `tests/swr_runtime.rs` closes for the plain functions.
//!
//! Unlike `swr_runtime.rs` (deliberately zero `node_modules`, proving
//! the plain functions need nothing installed), this test installs
//! real `devDependencies` (`react`, `swr`, `vitest`,
//! `@testing-library/react`, `jsdom`) via `npm install` — the point
//! here is exercising real React/SWR runtime behavior, not proving the
//! absence of a dependency. `tests/js/swr_hooks_invalidation/` holds
//! the checked-in `package.json`/`vitest.config.ts`/test file this
//! generates a package alongside and runs `npx vitest run` against.
//!
//! Skips (printed, not silently swallowed) when `node`/`npm`/`npx`
//! aren't on `PATH`, matching `swr_runtime.rs`'s rationale: no Rust CI
//! job in this repo currently provisions Node.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, TypeScriptPreset, generate_package};

#[test]
fn mutation_hooks_invalidate_exactly_the_documented_queries() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping mutation_hooks_invalidate_exactly_the_documented_queries: \
             `node`/`npm`/`npx` not on PATH (expected in this repo's Rust-only CI jobs — \
             see tests/swr_hooks_invalidation.rs's module doc)"
        );
        return;
    }

    let schema =
        cratestack_parser::parse_schema_file("tests/fixtures/swr_hooks_invalidation.cstack")
            .expect("fixture should parse");
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: "swr-hooks-invalidation-check".to_owned(),
            preset: TypeScriptPreset::Swr,
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .expect("swr preset should render");

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Overwrite the generated (framework-free, `swr`-less)
    // `package.json` with this test's fixture one, which declares the
    // real devDependencies vitest/@testing-library/react/jsdom/swr/
    // react need — the generated package.json intentionally carries
    // `swr`/`react` only as *peer* dependencies (AC #8), never as hard
    // deps, so it alone can't `npm install` a working test harness.
    let js_fixture_dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/js/swr_hooks_invalidation"
    );
    for asset in ["package.json", "vitest.config.ts", "invalidation.test.ts"] {
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
        "vitest run against the generated swr hooks failed — this is the real \
         invalidation-behavior proof for issue #305 AC #6/#7, not a Rust string assertion:\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    // Belt-and-suspenders: a successful process exit alone can't tell
    // an empty/skipped test run apart from a genuinely green one —
    // require vitest's own summary to report all four `it()` blocks
    // actually ran and passed.
    assert!(
        stdout.contains("4 passed") || stderr.contains("4 passed"),
        "expected vitest to report exactly 4 passed tests (the invalidation-proof suite):\n\
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
