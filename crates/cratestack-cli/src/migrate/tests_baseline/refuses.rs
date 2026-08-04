use std::fs;

use tempfile::TempDir;

use super::{WIDGET_SCHEMA, write_schema};
use crate::cli_types::BaselineBackendArg;
use crate::migrate::handle_baseline;

#[test]
fn refuses_to_run_when_snapshot_already_exists() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, WIDGET_SCHEMA);
    let out = dir.path().join("migrations");
    let backend_dir = out.join("postgres");
    fs::create_dir_all(&backend_dir).unwrap();
    fs::write(backend_dir.join("schema.snapshot.json"), "{}").unwrap();

    // A database URL that can never be reached — proves the refusal
    // happens before any network I/O, not just before any writes.
    let result = handle_baseline(
        schema,
        "postgres://unreachable-host-for-this-test:1/db".to_owned(),
        out,
        BaselineBackendArg::Postgres,
        false,
    );

    let err = result.expect_err("should refuse when a snapshot already exists");
    assert!(err.to_string().contains("already exists"));
}
