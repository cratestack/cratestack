#![cfg(test)]
//! `handle_diff` integration tests for validator CHECKs (cratestack#1129).

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::diff_cmd::handle_diff;
use crate::cli_types::MigrateBackendArg;

const PLAIN_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Member {
  id Int @id
  amount Int @range(min: 0, max: 1000000)
}
"#;

fn enforced_schema() -> String {
    PLAIN_SCHEMA.replace(
        "@range(min: 0, max: 1000000)",
        "@range(min: 0, max: 1000000) @db_enforce",
    )
}

fn migration_dirs(out: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(out.join("postgres"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// A first migration creates `members`, so its `@db_enforce` CHECK
/// cannot reject existing rows: no `up.pre.sql`, no WARNING.
#[test]
fn first_migration_with_validator_check_is_unscaffolded() {
    let dir = TempDir::new().expect("tempdir");
    let schema = dir.path().join("schema.cstack");
    fs::write(&schema, enforced_schema()).unwrap();
    let out = dir.path().join("migrations");

    handle_diff(
        schema,
        out.clone(),
        MigrateBackendArg::Postgres,
        "initial".to_owned(),
        false,
    )
    .expect("first diff");

    let dirs = migration_dirs(&out);
    assert_eq!(dirs.len(), 1);
    assert!(
        !dirs[0].join("up.pre.sql").exists(),
        "a first migration has no rows to back-fill"
    );
    let up = fs::read_to_string(dirs[0].join("up.sql")).unwrap();
    assert!(up.contains("CREATE TABLE members"));
    assert!(up.contains("members_amount_range_check"));
    assert!(!up.contains("WARNING"), "up.sql was: {up}");
}

/// Promoting `@db_enforce` onto a table that already exists is the case
/// the warning is for: blocking, with `up.pre.sql` scaffolded.
#[test]
fn promoting_db_enforce_on_existing_table_scaffolds_up_pre() {
    let dir = TempDir::new().expect("tempdir");
    let schema = dir.path().join("schema.cstack");
    fs::write(&schema, PLAIN_SCHEMA).unwrap();
    let out = dir.path().join("migrations");

    handle_diff(
        schema.clone(),
        out.clone(),
        MigrateBackendArg::Postgres,
        "initial".to_owned(),
        false,
    )
    .expect("first diff");

    fs::write(&schema, enforced_schema()).unwrap();
    handle_diff(
        schema,
        out.clone(),
        MigrateBackendArg::Postgres,
        "enforce".to_owned(),
        false,
    )
    .expect("second diff");

    let dirs = migration_dirs(&out);
    assert_eq!(dirs.len(), 2);
    // Two diffs in the same second share a timestamp; match by slug.
    let enforce = dirs
        .iter()
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_enforce"))
        })
        .expect("enforce migration");
    assert!(enforce.join("up.pre.sql").exists());
    let up = fs::read_to_string(enforce.join("up.sql")).unwrap();
    assert!(up.contains("WARNING"), "up.sql was: {up}");
}
