#![cfg(test)]
//! Integration tests for `cratestack generate-proto`, mirroring the style
//! of `cli_handlers/tests_generate.rs` (`--check` for Dart/TypeScript) —
//! plus the package-pinning rule from `docs/design/protobuf.md` §4.6 that
//! those two generators don't have.

use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::handle_generate_proto;

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

fn generate(
    schema: PathBuf,
    out: PathBuf,
    package: Option<&str>,
    check: bool,
) -> anyhow::Result<()> {
    handle_generate_proto(schema, out, package.map(str::to_owned), check)
}

#[test]
fn fresh_run_requires_package() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");

    let error = generate(schema, out, None, false).expect_err("first run needs --package");
    assert!(error.to_string().contains("--package is required"));
}

#[test]
fn second_run_without_package_reuses_the_locked_one() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");

    generate(schema.clone(), out.clone(), Some("shop_api"), false).expect("first run");
    generate(schema, out.clone(), None, false).expect("second run reuses locked package");

    assert!(
        fs::read_to_string(&out)
            .unwrap()
            .contains("package shop_api;")
    );
}

#[test]
fn package_mismatch_on_second_run_errors() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");

    generate(schema.clone(), out.clone(), Some("shop_api"), false).expect("first run");
    let error = generate(schema, out, Some("other_api"), false)
        .expect_err("changing package on a locked schema must error");
    assert!(error.to_string().contains("already pins `shop_api`"));
}

#[test]
fn matching_package_on_second_run_is_idempotent() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");

    generate(schema.clone(), out.clone(), Some("shop_api"), false).expect("first run");
    generate(schema, out, Some("shop_api"), false).expect("re-passing the same package is fine");
}

#[test]
fn normal_run_writes_lock_and_proto_at_expected_paths() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");
    let expected_lock = dir.path().join("schema.pb.lock");

    generate(schema, out.clone(), Some("shop_api"), false).expect("generate");

    assert!(
        expected_lock.exists(),
        "lock must sit beside the schema, not under --out"
    );
    assert!(out.exists());
    let proto = fs::read_to_string(&out).unwrap();
    assert!(proto.contains("message Account {"));
    assert!(proto.contains("package shop_api;"));
    let lock = fs::read_to_string(&expected_lock).unwrap();
    assert!(lock.contains("package = \"shop_api\""));
}

#[test]
fn check_on_clean_state_exits_ok_without_writing() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");
    let lock_path = dir.path().join("schema.pb.lock");

    generate(schema.clone(), out.clone(), Some("shop_api"), false).expect("initial generate");
    let proto_before = fs::read_to_string(&out).unwrap();
    let lock_before = fs::read_to_string(&lock_path).unwrap();

    generate(schema, out.clone(), None, true).expect("check should pass on unmodified output");

    assert_eq!(fs::read_to_string(&out).unwrap(), proto_before);
    assert_eq!(fs::read_to_string(&lock_path).unwrap(), lock_before);
}

#[test]
fn check_after_schema_edit_fails_and_does_not_write() {
    let dir = TempDir::new().expect("tempdir");
    let schema_path = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");
    let lock_path = dir.path().join("schema.pb.lock");

    generate(schema_path.clone(), out.clone(), Some("shop_api"), false).expect("initial generate");
    let proto_before = fs::read_to_string(&out).unwrap();
    let lock_before = fs::read_to_string(&lock_path).unwrap();

    fs::write(&schema_path, EXTENDED_SCHEMA).unwrap();

    let error = generate(schema_path, out.clone(), None, true)
        .expect_err("check should fail after a new field is added");
    assert!(error.to_string().contains("drift detected"));

    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        proto_before,
        "--check must not write the .proto file"
    );
    assert_eq!(
        fs::read_to_string(&lock_path).unwrap(),
        lock_before,
        "--check must not write the lock file"
    );
}

#[test]
fn check_on_first_run_with_no_existing_lock_reports_would_be_created() {
    let dir = TempDir::new().expect("tempdir");
    let schema = write_schema(&dir, INITIAL_SCHEMA);
    let out = dir.path().join("api.proto");

    let error = generate(schema, out.clone(), Some("shop_api"), true)
        .expect_err("check with no prior lock is drift");
    assert!(error.to_string().contains("would be created"));
    assert!(!out.exists(), "--check must never write");
}
