//! Compile-fail UI tests for the two `query`-block rejections
//! (cratestack#867; accepted design `docs/design/declarative-custom-query.md`).
//!
//! Both matter because the failure mode they replace is *silence*, not a
//! bad error message:
//!
//! - **Embedded** (§4): the embedded composer never iterates
//!   `schema.queries`, so without
//!   `include::embedded::query_guard` a `query` would simply not exist in
//!   the generated output, and the author would discover it as a missing
//!   method rather than as a rejected schema.
//! - **`db = None`**: the parser rejects a `query` under an explicit
//!   `datasource { provider = "none" }`, but a schema with *no*
//!   `datasource` block at all is legal and passes the existing datasource
//!   guard — so `db = None` plus no datasource block reaches codegen with
//!   queries intact and would emit `db.pool()` against the database-free
//!   `Cratestack`, producing a wall of "no method named `pool`" errors from
//!   inside a macro expansion.
//!
//! Note these are the *macro-level* rejections. The parse-time ones
//! (`$N` out of range, unknown result type, `@@embedded_sql` on a query,
//! `provider = "none"`, …) are unit-tested directly in
//! `cratestack-parser`'s `tests_queries.rs`, where the assertion can be on
//! the message rather than on a rustc snapshot.
//!
//! Fixture staging follows `ui.rs` exactly — read that file's module doc
//! for why the `.cstack` files are copied to a fixed-length absolute path
//! before being referenced. The staging dir constant must stay distinct
//! from the other UI drivers' so parallel test binaries don't race on the
//! same directory.

use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_STAGING_DIR: &str = "/tmp/cratestack-macros-ui-query-867";

#[test]
fn query_block_rejection_compile_fail() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let generated_dir = manifest_dir.join("tests/ui/generated");
    fs::create_dir_all(&generated_dir).expect("create tests/ui/generated");

    let staging_dir = Path::new(FIXTURE_STAGING_DIR);
    fs::create_dir_all(staging_dir).expect("create fixture staging dir");

    let t = trybuild::TestCases::new();

    let staged = stage_fixture(
        &manifest_dir,
        staging_dir,
        "tests/fixtures/query_embedded_rejected.cstack",
    );
    write_fixture(
        &generated_dir,
        "query_rejected_on_embedded.rs",
        &format!(
            "cratestack_macros::include_embedded_schema!({staged});\n\nfn main() {{}}\n",
            staged = path_str(&staged)
        ),
    );
    t.compile_fail(generated_dir.join("query_rejected_on_embedded.rs"));

    let staged = stage_fixture(
        &manifest_dir,
        staging_dir,
        "tests/fixtures/query_db_none_rejected.cstack",
    );
    write_fixture(
        &generated_dir,
        "query_rejected_under_db_none.rs",
        &format!(
            "cratestack_macros::include_server_schema!({staged}, db = None);\n\nfn main() {{}}\n",
            staged = path_str(&staged)
        ),
    );
    t.compile_fail(generated_dir.join("query_rejected_under_db_none.rs"));
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

fn write_fixture(generated_dir: &Path, file_name: &str, source: &str) {
    fs::write(generated_dir.join(file_name), source).expect("write generated trybuild fixture");
}
