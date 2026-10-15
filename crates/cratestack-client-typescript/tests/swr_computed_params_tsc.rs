//! Real-compiler proof for the `<Model>ComputedParams` import gap in the
//! `--swr` hooks (cratestack#1154). Before the fix, `image.hooks.ts`
//! referenced `ImageComputedParams` in its `list`/`get` hook signatures
//! without importing it, which `tsc --noEmit` rejects with
//! `TS2304: Cannot find name 'ImageComputedParams'`. A string assertion
//! alone can't tell "imports the type it uses" from "happens to contain
//! the substring", so this runs the TypeScript compiler against the
//! generated package, for both REST and RPC transports.
//!
//! The negative half (a model with no computed params gets no new import)
//! is checked by string on `widget.hooks.ts`, since `tsc` cannot tell an
//! unused import from an absent one.
//!
//! Follows `tests/swr_paged_model_tsc.rs`'s Node-availability skip
//! convention: it degrades to a printed skip rather than failing where
//! `node`/`npm`/`npx` are absent. CI runs this, because `ubuntu-latest`
//! ships Node.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn swr_computed_params_hooks_type_check() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping swr_computed_params_hooks_type_check: `node`/`npm`/`npx` not on PATH \
             (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this — see this test's module doc)"
        );
        return;
    }

    for fixture in ["swr_computed_params", "swr_computed_params_rpc"] {
        let schema =
            cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
                .unwrap_or_else(|error| panic!("fixture {fixture} should parse: {error}"));
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: format!("{fixture}-tsc-check").replace('_', "-"),
                swr: true,
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .unwrap_or_else(|error| panic!("{fixture}: --swr should render: {error}"));

        let dir = tempfile::tempdir().expect("tempdir");
        for file in &package.files {
            let path = dir.path().join(&file.file_name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dir");
            }
            fs::write(&path, &file.contents).expect("write generated file");
        }

        let install = Command::new("npm")
            .args([
                "install",
                "--no-save",
                "--no-audit",
                "--no-fund",
                "typescript@5",
                "swr",
            ])
            .current_dir(dir.path())
            .output()
            .expect("run npm install");
        assert!(
            install.status.success(),
            "{fixture}: npm install failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&install.stdout),
            String::from_utf8_lossy(&install.stderr)
        );

        let tsc = Command::new("npx")
            .args(["--yes", "tsc", "--noEmit", "-p", "tsconfig.json"])
            .current_dir(dir.path())
            .output()
            .expect("run npx tsc");

        let stdout = String::from_utf8_lossy(&tsc.stdout);
        let stderr = String::from_utf8_lossy(&tsc.stderr);
        assert!(
            !stdout.contains("Cannot find name 'ImageComputedParams'")
                && !stderr.contains("Cannot find name 'ImageComputedParams'"),
            "{fixture}: tsc reported a missing `ImageComputedParams` type — the exact regression \
             this test guards against:\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            tsc.status.success(),
            "{fixture}: tsc reported unexpected errors:\nstdout: {stdout}\nstderr: {stderr}"
        );

        // Negative half: a model with no computed params must not pick up
        // a new import. Checked after `tsc` so the TS2304 signal above is
        // the first failure a regression produces.
        let widget_hooks = fs::read_to_string(dir.path().join("src/swr/models/widget.hooks.ts"))
            .unwrap_or_else(|error| panic!("{fixture}: widget.hooks.ts should exist: {error}"));
        assert!(
            !widget_hooks.contains("ComputedParams"),
            "{fixture}: widget has no computed params, so widget.hooks.ts must not reference \
             or import any *ComputedParams type:\n{widget_hooks}"
        );
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
