//! The decisive proof for issue #617's AC4: a generated package with NO
//! flags builds and type-checks with `@tanstack/react-query` genuinely
//! ABSENT from `node_modules` — not merely undeclared in `package.json`.
//!
//! A string assertion that `package.json` doesn't mention the package (see
//! `tests/tanstack_generator.rs::without_the_flag_react_query_is_absent_everywhere_it_used_to_appear`)
//! cannot tell "the runtime import is gone" from "the import is still
//! there but the manifest entry that would have pulled its types in is
//! not" — `rest-react-query.ts.j2`/`rpc-react-query.ts.j2` open with a
//! *value* import (`useQuery`/`useMutation` called, not just used as
//! types), so the only thing that actually proves the dependency is gone
//! is a real `npm install` + `tsc` run against a `node_modules` that never
//! had the package fetched into it. This test does exactly that — real
//! `npm install`, a real assertion the package directory is absent from
//! the installed tree, and a real `npm run build` (`tsc -p tsconfig.json`).
//!
//! Follows this crate's established Node-availability skip convention
//! (`tests/swr_runtime.rs`, `tests/node_dist_esm.rs`, `tests/swr_paged_model_tsc.rs`):
//! it degrades to a printed skip rather than failing where `node`/`npm`/
//! `npx` are absent — a *local* Rust-only checkout, not CI, where
//! `ubuntu-latest` ships Node and this runs. Where Node IS available (this
//! session, and any future CI job that adds it), this is a real,
//! non-trivial verification — proven to actually discriminate by
//! temporarily re-adding the react-query template to the default REST
//! spec list and confirming this exact test fails against that build (see
//! this issue's PR description for that transcript; not committed here,
//! since a permanently re-broken generator would defeat the issue this
//! test exists to close).

use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn default_rest_package_installs_and_typechecks_without_tanstack_react_query_present() {
    run_for_fixture("tiny_rest", "tanstack-absent-rest-check");
}

#[test]
fn default_rpc_package_installs_and_typechecks_without_tanstack_react_query_present() {
    run_for_fixture("tiny_rpc", "tanstack-absent-rpc-check");
}

fn run_for_fixture(fixture_stem: &str, package_name: &str) {
    if !node_npm_available() {
        eprintln!(
            "skipping {package_name}: `node`/`npm` not on PATH (expected only where \
             Node is absent, e.g. a local Rust-only checkout; CI runs this — see this test's \
             module doc)"
        );
        return;
    }

    let schema =
        cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture_stem}.cstack"))
            .unwrap_or_else(|error| panic!("fixture {fixture_stem} should parse: {error}"));
    // Deliberately `TypeScriptGeneratorConfig::default()` with only
    // `package_name` overridden — NO flags at all. This is the exact
    // configuration AC4 describes: "a generated package with no flags".
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: default generation should succeed: {error}"));

    assert!(
        !package
            .files
            .iter()
            .any(|f| f.file_name == "src/react-query.ts"),
        "{fixture_stem}: src/react-query.ts must not be part of the default file set"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(&path, &file.contents).expect("write generated file");
    }

    // Real install, straight from the generated `package.json` — no
    // `--no-save` extra-package injection like `tests/swr_paged_model_tsc.rs`
    // uses for `swr`/`typescript`: the whole point here is that this
    // package's OWN manifest is what's installed, unmodified.
    let install = Command::new("npm")
        .args(["install", "--no-audit", "--no-fund"])
        .current_dir(dir.path())
        .output()
        .expect("run npm install");
    assert!(
        install.status.success(),
        "{fixture_stem}: npm install failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&install.stdout),
        String::from_utf8_lossy(&install.stderr)
    );

    // The actual discriminator: prove the package directory a value
    // import would need was never fetched, not just that `package.json`
    // doesn't mention it.
    let tanstack_dir = dir.path().join("node_modules/@tanstack/react-query");
    assert!(
        !tanstack_dir.exists(),
        "{fixture_stem}: @tanstack/react-query was installed into node_modules — the default \
         package.json must not pull it in at all:\n{}",
        tanstack_dir.display()
    );

    let build = Command::new("npm")
        .args(["run", "build"])
        .current_dir(dir.path())
        .output()
        .expect("run npm run build");
    assert!(
        build.status.success(),
        "{fixture_stem}: npm run build (tsc) failed WITHOUT --tanstack, with \
         @tanstack/react-query genuinely absent from node_modules — this is the exact proof \
         AC4 requires:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
}

fn node_npm_available() -> bool {
    ["node", "npm"].iter().all(|bin| {
        Command::new(bin)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}
