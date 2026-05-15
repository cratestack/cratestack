//! Committed snapshot of a `.cstack` schema as of the last generated
//! migration. The diff engine compares the current `.cstack` against
//! this snapshot to produce a new migration.
//!
//! The snapshot is written as pretty-printed JSON, one file per backend
//! (`migrations/postgres/schema.snapshot.json`,
//! `migrations/sqlite/schema.snapshot.json`). It must be committed to
//! source control — `cratestack migrate verify` is the CI gate that
//! confirms it hasn't been tampered with.

use std::fs;
use std::path::Path;

use cratestack_core::Schema;
use serde::{Deserialize, Serialize};

use crate::error::MigrateError;

/// Snapshot format version. Bump when the on-disk shape changes in a
/// way that requires regeneration. The diff engine refuses to operate
/// on snapshots whose `format_version` it does not understand.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;

/// Serialized form of a `.cstack` schema, plus metadata the diff engine
/// needs to interpret it correctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format_version: u32,
    pub schema: Schema,
}

impl Snapshot {
    pub fn from_schema(schema: Schema) -> Self {
        Self {
            format_version: SNAPSHOT_FORMAT_VERSION,
            schema,
        }
    }

    /// An empty snapshot — used as the "previous schema" when
    /// generating the very first migration for a backend.
    pub fn empty() -> Self {
        Self::from_schema(Schema {
            datasource: None,
            auth: None,
            config_blocks: Vec::new(),
            mixins: Vec::new(),
            models: Vec::new(),
            types: Vec::new(),
            enums: Vec::new(),
            procedures: Vec::new(),
            transport: Default::default(),
        })
    }
}

/// Read a snapshot file, or return [`Snapshot::empty`] if the file
/// does not exist. Used by the CLI to bootstrap the first migration
/// for a backend without forcing the developer to seed an empty
/// snapshot by hand. Any other I/O or parse failure propagates.
pub fn read_or_empty(path: impl AsRef<Path>) -> Result<Snapshot, MigrateError> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(Snapshot::empty());
    }
    read_snapshot(path)
}

/// Read and parse a snapshot file. Returns a structured error if the
/// file is missing, unparseable, or written by an incompatible
/// `cratestack-migrate` version.
pub fn read_snapshot(path: impl AsRef<Path>) -> Result<Snapshot, MigrateError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| MigrateError::SnapshotRead {
        path: path.to_path_buf(),
        source,
    })?;
    let snapshot: Snapshot =
        serde_json::from_slice(&bytes).map_err(|source| MigrateError::SnapshotParse {
            path: path.to_path_buf(),
            source,
        })?;
    if snapshot.format_version != SNAPSHOT_FORMAT_VERSION {
        return Err(MigrateError::SnapshotFormatVersion {
            path: path.to_path_buf(),
            found: snapshot.format_version,
            expected: SNAPSHOT_FORMAT_VERSION,
        });
    }
    Ok(snapshot)
}

/// Write a snapshot to disk as pretty-printed JSON with a trailing
/// newline (so diff tools and editors handle the file cleanly).
pub fn write_snapshot(snapshot: &Snapshot, path: impl AsRef<Path>) -> Result<(), MigrateError> {
    let path = path.as_ref();
    let mut json = serde_json::to_string_pretty(snapshot).map_err(MigrateError::SnapshotSerialize)?;
    json.push('\n');
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|source| MigrateError::SnapshotWrite {
            path: path.to_path_buf(),
            source,
        })?;
    }
    fs::write(path, json).map_err(|source| MigrateError::SnapshotWrite {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cratestack_parser::parse_schema;
    use tempfile::TempDir;

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
        let snapshot = Snapshot::from_schema(schema);

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("schema.snapshot.json");
        write_snapshot(&snapshot, &path).expect("write");

        let loaded = read_snapshot(&path).expect("read");
        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn write_creates_missing_parent_directories() {
        let schema = parse(TINY_SCHEMA);
        let snapshot = Snapshot::from_schema(schema);

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
        let snapshot = Snapshot::from_schema(schema);

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
        let mut snapshot = Snapshot::from_schema(schema);
        snapshot.format_version = SNAPSHOT_FORMAT_VERSION + 99;

        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("schema.snapshot.json");
        write_snapshot(&snapshot, &path).expect("write");

        let err = read_snapshot(&path).expect_err("should reject");
        match err {
            MigrateError::SnapshotFormatVersion { found, expected, .. } => {
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
}
