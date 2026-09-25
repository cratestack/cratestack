//! Compile-fail UI tests for the MCP gate (`include/mcp_gate.rs`, ADR 0002
//! Q4/Q8/D3; cratestack#1036, cratestack#1038).
//!
//! This crate's tests build *without* its `mcp` feature (no dev-dependency
//! turns it on), so what these pin is the feature-off behaviour, plus the
//! refusals that hold whatever the feature state:
//!
//! - `include_server_schema!` without the `mcp` feature fails asking for it,
//!   for both server shapes (`db = Postgres` with tools and resources,
//!   `db = None` with tools only). Phase 1's message named cratestack#1033
//!   instead; phase 3 made the schemas servable, so the message changed.
//!   The feature-on side is not reachable from here: it is what the
//!   facades' `mcp` suites (`cratestack-api`'s `tests/mcp_*.rs`) compile,
//!   and `include/mcp_gate/tests.rs` drives the decision in both states.
//! - `include_embedded_schema!` fails citing ADR 0002 D3, permanently.
//! - `@mcp(tool)` on a `@stream` procedure fails naming Q8 (a parser rule).
//! - `@mcp(tool)` on a procedure taking `Json` fails naming the missing
//!   JSON Schema mapping, before the feature check.
//! - `@@mcp(resource: ...)` on a model whose `@@internal` hides a read verb
//!   fails naming the contradiction, before the feature check
//!   (cratestack#1040). The resource *being served* with the feature on is
//!   what `cratestack-pg`'s `tests/mcp_resources_pg.rs` compiles.
//!
//! The third role, `include_client_schema!`, *accepts* an MCP schema; that
//! is a pass case, so it lives in `cratestack-client`'s
//! `tests/mcp_declarations_are_ignored.rs`, where the generated client can
//! actually be exercised.
//!
//! Fixture staging follows `ui.rs` exactly — read that file's module doc for
//! why the `.cstack` files are copied to a fixed-length absolute path first.
//! The staging dir must stay distinct from the other UI drivers' so parallel
//! test binaries don't race on it.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_STAGING_DIR: &str = "/tmp/cratestack-macros-ui-mcp-1036";

#[test]
fn mcp_release_gate_compile_fail() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = manifest_dir.join("tests/ui/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/ui/generated");

    let staging_dir = Path::new(FIXTURE_STAGING_DIR);
    fs::create_dir_all(staging_dir).expect("create fixture staging dir");

    let t = trybuild::TestCases::new();
    let cases = [
        (
            "mcp_gated_on_server.rs",
            "tests/fixtures/mcp_server_gate.cstack",
            "include_server_schema!({staged}, db = Postgres)",
        ),
        (
            "mcp_gated_on_server_db_none.rs",
            "tests/fixtures/mcp_server_gate_db_none.cstack",
            "include_server_schema!({staged}, db = None)",
        ),
        (
            "mcp_rejected_on_embedded.rs",
            "tests/fixtures/mcp_embedded_rejected.cstack",
            "include_embedded_schema!({staged})",
        ),
        (
            "mcp_stream_tool_refused.rs",
            "tests/fixtures/mcp_stream_tool.cstack",
            "include_server_schema!({staged}, db = None)",
        ),
        (
            "mcp_json_tool_refused.rs",
            "tests/fixtures/mcp_json_tool.cstack",
            "include_server_schema!({staged}, db = None)",
        ),
        (
            "mcp_resource_internal_refused.rs",
            "tests/fixtures/mcp_resource_internal.cstack",
            "include_server_schema!({staged}, db = Postgres)",
        ),
    ];
    for (file_name, fixture, call) in cases {
        let staged = stage_fixture(&manifest_dir, staging_dir, fixture);
        let call = call.replace("{staged}", &path_str(&staged));
        let source = format!("cratestack_macros::{call};\n\nfn main() {{}}\n");
        fs::write(generated_dir.join(file_name), source).expect("write generated fixture");
        t.compile_fail(generated_dir.join(file_name));
    }
}

fn path_str(path: &Path) -> String {
    format!(
        "{:?}",
        path.to_str().expect("staging path should be valid UTF-8")
    )
}

fn stage_fixture(manifest_dir: &Path, staging_dir: &Path, relative_schema_path: &str) -> PathBuf {
    let source = manifest_dir.join(relative_schema_path);
    let file_name = source
        .file_name()
        .expect("fixture schema path should have a file name");
    let staged = staging_dir.join(file_name);
    fs::copy(&source, &staged).unwrap_or_else(|error| {
        panic!(
            "copy fixture {} to {}: {error}",
            source.display(),
            staged.display()
        )
    });
    staged
}
