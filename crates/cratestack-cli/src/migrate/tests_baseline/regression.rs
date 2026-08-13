use std::fs;

use tempfile::TempDir;

use super::{
    WIDGET_SCHEMA, WIDGET_SCHEMA_WITH_DESCRIPTION, block_on, connect, exec, isolated_test_db,
    migration_dirs, write_schema,
};
use crate::cli_types::{BaselineBackendArg, MigrateBackendArg};
use crate::migrate::{handle_baseline, handle_diff};

/// Design doc §8 case 3 — the regression test for issue #135 as
/// originally reported: baseline an existing database, then add a
/// field to the schema, and confirm `migrate diff` emits an
/// incremental `ALTER TABLE`, not a `CREATE TABLE` (which is what
/// happened before baselining existed, since `diff` had no way to
/// know the table already existed).
#[test]
fn clean_baseline_then_added_field_produces_alter_table_not_create_table() {
    let Some(isolated_url) = isolated_test_db("regression") else {
        return;
    };

    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, WIDGET_SCHEMA);
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
        schema_path.clone(),
        isolated_url,
        out.clone(),
        BaselineBackendArg::Postgres,
        false,
    )
    .expect("clean baseline should succeed");

    fs::write(&schema_path, WIDGET_SCHEMA_WITH_DESCRIPTION).unwrap();

    handle_diff(
        schema_path,
        out.clone(),
        MigrateBackendArg::Postgres,
        "add_description".to_owned(),
        false,
    )
    .expect("diff after baseline should succeed");

    let backend_dir = out.join("postgres");
    let migrations = migration_dirs(&backend_dir);
    assert_eq!(
        migrations.len(),
        1,
        "exactly one migration should be generated for the added field"
    );
    let up = fs::read_to_string(migrations[0].join("up.sql")).unwrap();
    assert!(
        up.contains("ALTER TABLE widgets ADD COLUMN description"),
        "expected an incremental ALTER TABLE, got:\n{up}"
    );
    assert!(
        !up.contains("CREATE TABLE"),
        "baseline should have made the table itself a non-event, got:\n{up}"
    );
}
