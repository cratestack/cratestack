//! Compile-fail UI tests for the MCP release gate (`include/mcp_gate.rs`,
//! ADR 0002 Q4/D3, cratestack#1036).
//!
//! What these pin is that a valid MCP schema cannot build today:
//!
//! - `include_server_schema!` fails naming cratestack#1033, for both server
//!   shapes (`db = Postgres` with tools and resources, `db = None` with tools
//!   only). Phase 3 removes this gate and must replace these cases with
//!   passing ones — the snapshot changing is the signal, not an accident.
//! - `include_embedded_schema!` fails citing ADR 0002 D3, permanently.
//!
//! The third role, `include_client_schema!`, *accepts* the same schema; that
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
