//! Real-compiler proof for the `FindMany<Model>` procedure-argument
//! import gap that `tests/swr_generator.rs`'s
//! `find_many_procedure_argument_imports_post_find_many_in_every_file_that_uses_it`
//! checks by string — see `swr_paged_model_tsc.rs`'s module doc for why a
//! string assertion alone can't tell "imports the type it uses" from
//! "happens to contain the substring", and follows that file's structure
//! (and Node-availability skip convention) verbatim, swapped to the
//! `FindMany` fixtures.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn find_many_procedure_output_type_checks() {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping find_many_procedure_output_type_checks: `node`/`npm`/`npx` not on PATH \
             (expected only where Node is absent, e.g. a local Rust-only checkout; CI runs this — see this test's module doc)"
        );
        return;
    }

    for fixture in ["swr_find_many_procedure", "swr_find_many_procedure_rpc"] {
        let schema =
            cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture}.cstack"))
                .unwrap_or_else(|error| panic!("fixture {fixture} should parse: {error}"));
        let package = generate_package(
            &schema,
            &TypeScriptGeneratorConfig {
                package_name: "swr-find-many-procedure-tsc-check".to_owned(),
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
            !stdout.contains("Cannot find name 'PostFindMany'")
                && !stderr.contains("Cannot find name 'PostFindMany'"),
            "{fixture}: tsc reported a missing `PostFindMany` type — the exact regression this \
             test guards against:\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(
            tsc.status.success(),
            "{fixture}: tsc reported unexpected errors:\nstdout: {stdout}\nstderr: {stderr}"
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
