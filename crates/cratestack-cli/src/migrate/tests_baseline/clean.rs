use tempfile::TempDir;

use super::{
    WIDGET_SCHEMA, block_on, connect, exec, isolated_test_db, migration_dirs, write_schema,
};
use crate::cli_types::{BaselineBackendArg, MigrateBackendArg};
use crate::migrate::{handle_baseline, handle_diff};

#[test]
fn clean_baseline_reports_no_drift_and_subsequent_diff_reports_no_changes() {
    let Some(isolated_url) = isolated_test_db("clean") else {
        return;
    };

    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, WIDGET_SCHEMA);
    let out = dir.path().join("migrations");

    block_on(async {
        let pool = connect(&isolated_url).await;
        exec(
            &pool,
            "CREATE TABLE widgets (id BIGINT NOT NULL PRIMARY KEY, name TEXT NOT NULL); \
             CREATE UNIQUE INDEX widgets_name_key ON widgets (name);",
        )
        .await;
    });

    handle_baseline(
        schema.clone(),
        isolated_url,
        out.clone(),
        BaselineBackendArg::Postgres,
        false,
    )
    .expect("clean baseline should succeed");

    let backend_dir = out.join("postgres");
    assert!(backend_dir.join("schema.snapshot.json").exists());

    // "a subsequent `migrate diff` reports no pending changes" —
    // observable as: no migration directory gets written, since
    // `diff_cmd` only ever writes one when the op list is non-empty.
    handle_diff(
        schema,
        out.clone(),
        MigrateBackendArg::Postgres,
        "after_baseline".to_owned(),
        false,
    )
    .expect("diff after a clean baseline should succeed");
    assert!(
        migration_dirs(&backend_dir).is_empty(),
        "a clean baseline should leave nothing for `migrate diff` to generate"
    );
}
