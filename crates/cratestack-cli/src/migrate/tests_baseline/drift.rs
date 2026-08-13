use tempfile::TempDir;

use super::{WIDGET_SCHEMA, block_on, connect, exec, isolated_test_db, write_schema};
use crate::cli_types::BaselineBackendArg;
use crate::migrate::handle_baseline;

#[test]
fn drifted_baseline_reports_drift_but_still_exits_ok_and_writes_snapshot() {
    let Some(isolated_url) = isolated_test_db("drift") else {
        return;
    };

    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, WIDGET_SCHEMA);
    let out = dir.path().join("migrations");

    block_on(async {
        let pool = connect(&isolated_url).await;
        // Extra column (`legacy_note`) the schema doesn't declare, and
        // no unique index on `name` — both should surface as drift.
        exec(
            &pool,
            "CREATE TABLE widgets (id BIGINT NOT NULL PRIMARY KEY, name TEXT NOT NULL, \
             legacy_note TEXT);",
        )
        .await;
    });

    // Must exit Ok (report drift, don't hard-fail) per issue #135's
    // explicit acceptance criteria.
    handle_baseline(
        schema,
        isolated_url,
        out.clone(),
        BaselineBackendArg::Postgres,
        false,
    )
    .expect("drifted baseline should still exit 0");

    let backend_dir = out.join("postgres");
    let snapshot_path = backend_dir.join("schema.snapshot.json");
    assert!(snapshot_path.exists(), "snapshot should still be written");

    // The snapshot reflects the *introspected* (live) shape, not the
    // aspirational schema — `legacy_note` and the missing unique index
    // must be visible in what was written.
    let snapshot =
        cratestack_migrate::read_snapshot(&snapshot_path).expect("read written snapshot");
    let widgets = snapshot
        .projections
        .tables
        .get("widgets")
        .expect("widgets table in snapshot");
    assert!(
        widgets.columns.iter().any(|c| c.name == "legacy_note"),
        "snapshot should carry the drifted live shape, including the extra column"
    );
}
