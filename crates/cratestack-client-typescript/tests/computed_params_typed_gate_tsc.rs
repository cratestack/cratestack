//! Real-compiler proof for the typed, per-model-gated `computedParams`
//! surface — see `docs/design/computed-fields.md`'s "Downstream" section.
//!
//! A string assertion on the generated source (see `tests/generator.rs`'s
//! `model_computed_field_is_response_only_and_computed_params_is_available_on_reads`)
//! can prove the generic default is `never`, but it can't prove `tsc`
//! actually REJECTS `computedParams` on an ungated model — only a real
//! compiler run can. This test drops a smoke file into the generated
//! package's own `src/` (so `tsconfig.json`'s `"include": ["src/**/*.ts"]`
//! picks it up) that:
//!
//!   - assigns `computedParams` on `Image` (gated: declares
//!     `proxyUrl String @computed(params: ProxyParams?)`) with NO
//!     `@ts-expect-error` — this must type-check cleanly, proving the gate
//!     doesn't also reject the *legitimate* case;
//!   - assigns `computedParams` on `Widget` (ungated: no computed fields
//!     at all) immediately preceded by `// @ts-expect-error` — `tsc`
//!     fails the whole build with `TS2578: Unused '@ts-expect-error'
//!     directive` if that line does NOT actually produce an error, so a
//!     successful `tsc --noEmit` run here is the decisive proof the
//!     per-model gate is real, not just documentation.
//!
//! Covers both REST (`computed_params.cstack`) and RPC
//! (`computed_params_rpc.cstack`) — the RPC client's own `get`/`list`
//! gate (`{{ model.api_name }}GetOptions`/`CratestackRpcListQuery<T>`)
//! needs its own proof, since it's a structurally different mechanism
//! from REST's `CratestackQueryRequestConfig<T>`.
//!
//! Follows this crate's established Node-availability skip convention
//! (`tests/swr_runtime.rs`, `tests/swr_paged_model_tsc.rs`,
//! `tests/tanstack_absent_typechecks.rs`): it degrades to a printed skip
//! rather than failing where `node`/`npm`/`npx` are absent. That is a
//! *local* Rust-only checkout — in CI this runs, because `ubuntu-latest`
//! ships Node.

use std::fs;
use std::process::Command;

use cratestack_client_typescript::{TypeScriptGeneratorConfig, generate_package};

#[test]
fn rest_computed_params_gate_is_enforced_by_tsc() {
    run_for_fixture(
        "computed_params",
        "computed-params-tsc-rest-check",
        r#"
import { ComputedParamsTscRestCheckClient } from "./client.js";

declare const client: ComputedParamsTscRestCheckClient;

// Gated model: Image declares a parameterized computed field, so
// `computedParams` is a real, typed option — this must type-check clean.
void client.images.list({ query: { computedParams: { proxyUrl: { width: 800 } } } });

// Ungated model: Widget declares no computed fields at all, so
// `computedParams` must be unassignable (`never`) — `tsc` must reject
// this, and `@ts-expect-error` must actually be consumed.
// @ts-expect-error computedParams is not assignable on an ungated model
void client.widgets.list({ query: { computedParams: { anything: 1 } } });
"#,
    );
}

#[test]
fn rpc_computed_params_gate_is_enforced_by_tsc() {
    run_for_fixture(
        "computed_params_rpc",
        "computed-params-tsc-rpc-check",
        r#"
import { ComputedParamsTscRpcCheckClient } from "./client.js";

declare const client: ComputedParamsTscRpcCheckClient;

// Gated model: `list`'s query and `get`'s options both accept a typed
// `computedParams` — must type-check clean.
void client.images.list({ computedParams: { proxyUrl: { width: 800 } } });
void client.images.get(1, { computedParams: { proxyUrl: { width: 800 } } });

// Ungated model: neither call site accepts `computedParams` at all.
// @ts-expect-error computedParams is not assignable on an ungated model's list query
void client.widgets.list({ computedParams: { anything: 1 } });
// @ts-expect-error computedParams is not a property of the ungated model's get options
void client.widgets.get(1, { computedParams: { anything: 1 } });
"#,
    );
}

fn run_for_fixture(fixture_stem: &str, package_name: &str, smoke_source: &str) {
    if !node_npm_npx_available() {
        eprintln!(
            "skipping {package_name}: `node`/`npm`/`npx` not on PATH (expected only where \
             Node is absent, e.g. a local Rust-only checkout; CI runs this — see this test's \
             module doc)"
        );
        return;
    }

    let schema =
        cratestack_parser::parse_schema_file(format!("tests/fixtures/{fixture_stem}.cstack"))
            .unwrap_or_else(|error| panic!("fixture {fixture_stem} should parse: {error}"));
    let package = generate_package(
        &schema,
        &TypeScriptGeneratorConfig {
            package_name: package_name.to_owned(),
            ..TypeScriptGeneratorConfig::default()
        },
    )
    .unwrap_or_else(|error| panic!("{fixture_stem}: default generation should succeed: {error}"));

    let dir = tempfile::tempdir().expect("tempdir");
    for file in &package.files {
        let path = dir.path().join(&file.file_name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&path, &file.contents).expect("write generated file");
    }

    // Dropped inside `src/` (not the package root) so `tsconfig.json`'s
    // `"include": ["src/**/*.ts"]` picks it up under a plain `tsc -p`.
    fs::write(dir.path().join("src/smoke.ts"), smoke_source).expect("write smoke script");

    // `typescript` is already an unconditional `devDependencies` entry
    // (`crate::package_deps::dev_dependencies_for`), so a plain install
    // off the generated manifest is enough — no extra `--no-save`
    // packages needed, unlike `tests/swr_paged_model_tsc.rs`'s `swr`.
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

    let tsc = Command::new("npx")
        .args(["--yes", "tsc", "--noEmit", "-p", "tsconfig.json"])
        .current_dir(dir.path())
        .output()
        .expect("run npx tsc");

    assert!(
        tsc.status.success(),
        "{fixture_stem}: tsc rejected the generated package — either the gated model's \
         legitimate computedParams usage failed to type-check, or the ungated model's \
         @ts-expect-error went unused (meaning the gate did NOT reject computedParams there):\n\
         stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&tsc.stdout),
        String::from_utf8_lossy(&tsc.stderr)
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
