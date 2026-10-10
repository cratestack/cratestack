use std::fs;

use cratestack_parser::parse_schema;
use tempfile::TempDir;

use super::*;

const TINY_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Account {
  id Int @id
  balance Int
}
"#;

fn parse(source: &str) -> Schema {
    parse_schema(source).expect("schema should parse")
}

#[test]
fn snapshot_round_trips_through_disk() {
    let schema = parse(TINY_SCHEMA);
    let snapshot = Snapshot::from_schema(&schema);

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("schema.snapshot.json");
    write_snapshot(&snapshot, &path).expect("write");

    let loaded = read_snapshot(&path).expect("read");
    assert_eq!(loaded, snapshot);
}

#[test]
fn write_creates_missing_parent_directories() {
    let schema = parse(TINY_SCHEMA);
    let snapshot = Snapshot::from_schema(&schema);

    let dir = TempDir::new().expect("tempdir");
    let path = dir
        .path()
        .join("migrations")
        .join("postgres")
        .join("schema.snapshot.json");
    write_snapshot(&snapshot, &path).expect("write through missing parents");
    assert!(path.exists());
}

#[test]
fn write_emits_pretty_json_with_trailing_newline() {
    let schema = parse(TINY_SCHEMA);
    let snapshot = Snapshot::from_schema(&schema);

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("schema.snapshot.json");
    write_snapshot(&snapshot, &path).expect("write");

    let contents = fs::read_to_string(&path).expect("read text");
    assert!(contents.ends_with('\n'), "snapshot should end with newline");
    assert!(
        contents.contains('\n') && contents.contains("  "),
        "snapshot should be pretty-printed"
    );
}

#[test]
fn read_rejects_incompatible_format_version() {
    let schema = parse(TINY_SCHEMA);
    let mut snapshot = Snapshot::from_schema(&schema);
    snapshot.format_version = SNAPSHOT_FORMAT_VERSION + 99;

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("schema.snapshot.json");
    write_snapshot(&snapshot, &path).expect("write");

    let err = read_snapshot(&path).expect_err("should reject");
    match err {
        MigrateError::SnapshotFormatVersion {
            found, expected, ..
        } => {
            assert_eq!(found, SNAPSHOT_FORMAT_VERSION + 99);
            assert_eq!(expected, SNAPSHOT_FORMAT_VERSION);
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn read_reports_missing_file_as_structured_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("does-not-exist.json");
    let err = read_snapshot(&path).expect_err("missing should fail");
    assert!(matches!(err, MigrateError::SnapshotRead { .. }));
}

#[test]
fn read_reports_malformed_json_as_structured_error() {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("schema.snapshot.json");
    fs::write(&path, b"{not valid json").expect("write garbage");

    let err = read_snapshot(&path).expect_err("malformed should fail");
    assert!(matches!(err, MigrateError::SnapshotParse { .. }));
}

const BIGINT_SCHEMA: &str = r#"
datasource db {
  provider = "postgresql"
  url = env("DATABASE_URL")
}

model Ledger {
  id BigInt @id
  amountE8 BigInt
  feeE8 BigInt?
}
"#;

/// The snapshot keeps the `.cstack` scalar name as a string
/// (`ColumnType::Scalar`), so `BigInt` needs no format change, but it
/// must survive the disk round trip as `BigInt`: a snapshot that came
/// back as `Int` (or anything else) would make the next `migrate diff`
/// invent a type change.
#[test]
fn bigint_columns_round_trip_through_the_snapshot_unchanged() {
    let schema = parse(BIGINT_SCHEMA);
    let snapshot = Snapshot::from_schema(&schema);

    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("schema.snapshot.json");
    write_snapshot(&snapshot, &path).expect("write");
    let contents = fs::read_to_string(&path).expect("read text");
    assert!(
        contents.contains("\"BigInt\""),
        "the snapshot should record the BigInt scalar name: {contents}"
    );

    let loaded = read_snapshot(&path).expect("read");
    assert_eq!(loaded, snapshot);

    let ops = crate::diff::diff_projections(&loaded.projections, &project(&schema))
        .expect("diff should succeed");
    assert!(
        ops.is_empty(),
        "a BigInt schema diffed against its own snapshot must be a no-op: {ops:?}"
    );
}
