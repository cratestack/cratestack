//! Real-compiler proof for the `@@paged` model / `Page<T>`-returning
//! procedure import gap that `tests/swr_generator.rs`'s
//! `paged_model_imports_page_in_every_file_that_uses_it` checks by
//! string. Before the fix, every per-model/procedures file (and their
//! `.hooks.ts` siblings) used `Page<Widget>` in a type position with no
//! `import type { Page }` anywhere, which `tsc --noEmit` rejects with
//! `TS2304: Cannot find name 'Page'` — a string assertion on the
//! generated source can't tell "imports the type it uses" from
//! "happens to contain the substring `Page<Widget>`", so this actually
//! runs the TypeScript compiler against the generated package.
//!
//! Follows `tests/swr_runtime.rs`'s Node-availability skip convention:
//! no Rust CI job in this repo currently provisions Node, so this
//! degrades to a printed skip rather than failing a job that was never
//! going to have `node`/`npm`/`npx` on `PATH`.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, TypeScriptPreset, generate_package};

#[test]
fn paged_model_output_type_checks() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping paged_model_output_type_checks: `node`/`npm`/`npx` not on PATH \
             (expected in this repo's Rust-only CI jobs — see this test's module doc)"
        );
        return;
    }

    for fixture in ["swr_paged_model", "swr_paged_model_rpc"] {
        let schema =
            cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
                .unwrap_or_else(|error| panic!("fixture {fixture} should parse: {error}"));
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: "swr-paged-model-tsc-check".to_owned(),
                preset: TypeScriptPreset::Swr,
                ..TypeScriptGeneratorConfig::default()
            },
        )
        .unwrap_or_else(|error| panic!("{fixture}: swr preset should render: {error}"));

        let dir = tempfile::tempdir().expect("tempdir");
        for file in &package.files {
            let path = dir.path().join(&file.file_name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dir");
            }
            fs::write(&path, &file.contents).expect("write generated file");
        }

        // The generated `package.json` carries `swr`/`typescript` as
        // peer/dev deps only (never hard deps — see
        // `swr_package_json_declares_swr_and_react_as_peer_dependencies`),
        // so install them directly rather than relying on the generated
        // manifest to pull them in.
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
            !stdout.contains("Cannot find name 'Page'")
                && !stderr.contains("Cannot find name 'Page'"),
            "{fixture}: tsc reported a missing `Page` type — the exact regression this test \
             guards against:\nstdout: {stdout}\nstderr: {stderr}"
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
