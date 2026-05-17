#![cfg(test)]
//! Integration tests for `handle_diff`.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::diff_cmd::handle_diff;
use crate::cli_types::MigrateBackendArg;

fn write_schema(dir: &TempDir, source: &str) -> PathBuf {
    let path = dir.path().join("schema.cstack");
    fs::write(&path, source).expect("write schema");
    path
}

const INITIAL_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
}
"#;

const EXTENDED_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
  note String?
}
"#;

#[test]
fn diff_writes_initial_migration_and_snapshot() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("migrations");

    handle_diff(
        schema,
        out.clone(),
        MigrateBackendArg::Postgres,
        "initial".to_owned(),
        false,
    )
    .expect("diff");

    let backend_dir = out.join("postgres");
    assert!(backend_dir.join("schema.snapshot.json").exists());

    // Exactly one migration directory created.
    let entries: Vec<_> = fs::read_dir(&backend_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(entries.len(), 1);
    let migration_dir = &entries[0];
    let up = fs::read_to_string(migration_dir.join("up.sql")).unwrap();
    assert!(up.contains("CREATE TABLE accounts"));
}

#[test]
fn second_diff_is_incremental() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("migrations");

    handle_diff(
        schema_path.clone(),
        out.clone(),
        MigrateBackendArg::Postgres,
        "initial".to_owned(),
        false,
    )
    .expect("first diff");

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    handle_diff(
        schema_path,
        out.clone(),
        MigrateBackendArg::Postgres,
        "add_note".to_owned(),
        false,
    )
    .expect("second diff");

    let backend_dir = out.join("postgres");
    let migrations: Vec<_> = fs::read_dir(&backend_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(migrations.len(), 2);

    // Two diffs run within the same second share a timestamp, so
    // disambiguate by slug rather than relying on sort order.
    let add_note = migrations
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with("_add_note"))
                .unwrap_or(false)
        })
        .expect("add_note migration");
    let up = fs::read_to_string(add_note.join("up.sql")).unwrap();
    assert!(up.contains("ALTER TABLE accounts ADD COLUMN note TEXT"));
    assert!(!up.contains("CREATE TABLE"));
}

#[test]
fn destructive_diff_requires_flag() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, EXTENDED_SCHEMA);
    let out = dir.path().join("migrations");

    handle_diff(
        schema_path.clone(),
        out.clone(),
        MigrateBackendArg::Postgres,
        "initial".to_owned(),
        false,
    )
    .expect("first diff");

    fs::write(&schema_path, INITIAL_SCHEMA).unwrap();

    let result = handle_diff(
        schema_path.clone(),
        out.clone(),
        MigrateBackendArg::Postgres,
        "drop_note".to_owned(),
        false,
    );
    let err = result.expect_err("should refuse destructive without flag");
    assert!(err.to_string().contains("--allow-destructive"));

    // With the flag set, the same diff succeeds.
    handle_diff(
        schema_path,
        out,
        MigrateBackendArg::Postgres,
        "drop_note".to_owned(),
        true,
    )
    .expect("destructive with flag");
}

#[test]
fn both_backends_produce_separate_trees() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("migrations");

    handle_diff(
        schema,
        out.clone(),
        MigrateBackendArg::Both,
        "initial".to_owned(),
        false,
    )
    .expect("both diff");

    assert!(out.join("postgres").join("schema.snapshot.json").exists());
    assert!(out.join("sqlite").join("schema.snapshot.json").exists());

    let pg_entries: Vec<_> = fs::read_dir(out.join("postgres"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    let sqlite_entries: Vec<_> = fs::read_dir(out.join("sqlite"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    assert_eq!(pg_entries.len(), 1);
    assert_eq!(sqlite_entries.len(), 1);

    let pg_up = fs::read_to_string(pg_entries[0].join("up.sql")).unwrap();
    let sqlite_up = fs::read_to_string(sqlite_entries[0].join("up.sql")).unwrap();
    assert!(pg_up.contains("BIGINT"));
    assert!(sqlite_up.contains("BLOB"));
}
