//! Committed snapshot of a schema's SQL shape as of the last generated
//! migration (or, since issue #205, the last `migrate baseline` run).
//! The diff engine compares the current `.cstack` against this
//! snapshot to produce a new migration.
//!
//! The snapshot is written as pretty-printed JSON, one file per
//! backend (`migrations/postgres/schema.snapshot.json`,
//! `migrations/sqlite/schema.snapshot.json`). It must be committed
//! to source control — `cratestack migrate verify` is the CI gate
//! that confirms it hasn't been tampered with.
//!
//! **Format version 2 (issue #205):** the snapshot stores
//! [`Projections`] — the backend-agnostic IR — rather than a full
//! `cratestack_core::Schema` as format version 1 did. A `Schema` can't
//! represent what `cratestack migrate baseline` establishes as a
//! starting point: introspecting a live database recovers table/
//! column/index/check shape, never `mixins`, `procedures`, `auth`, or
//! attribute provenance (design doc `docs/design/migrate-baseline.md`
//! §5.3), so there is no `Schema` a baseline run could honestly write.
//! Storing the IR directly removes that mismatch — `cratestack migrate
//! diff` and `cratestack migrate baseline` now write the exact same
//! shape, and the "previous state" for a diff is always literally the
//! SQL shape currently on record, not an aspirational schema. This is
//! a breaking on-disk format change with no migration path: version-1
//! snapshots are rejected (see [`read_snapshot`]) and must be
//! regenerated. Pre-1.0, so no compatibility shim is provided.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::Path;

use cratestack_core::Schema;
use serde::{Deserialize, Serialize};

use crate::error::MigrateError;
use crate::projection::{Projections, project};

/// Snapshot format version. Bump when the on-disk shape changes in a
/// way that requires regeneration. The diff engine refuses to operate
/// on snapshots whose `format_version` it does not understand.
pub const SNAPSHOT_FORMAT_VERSION: u32 = 2;

/// Serialized "previous state" the diff engine compares against: the
/// [`Projections`] IR, plus metadata needed to interpret it correctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub format_version: u32,
    pub projections: Projections,
}

impl Snapshot {
    pub fn from_projections(projections: Projections) -> Self {
        Self {
            format_version: SNAPSHOT_FORMAT_VERSION,
            projections,
        }
    }

    /// Project a parsed `.cstack` [`Schema`] and wrap it as a
    /// snapshot — the path `cratestack migrate diff` uses after
    /// writing a migration, so the next diff starts from "the schema
    /// as of the last generated migration."
    pub fn from_schema(schema: &Schema) -> Self {
        Self::from_projections(project(schema))
    }

    /// An empty snapshot — used as the "previous state" when
    /// generating the very first migration for a backend.
    pub fn empty() -> Self {
        Self::from_projections(Projections::default())
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
    let mut json =
        serde_json::to_string_pretty(snapshot).map_err(MigrateError::SnapshotSerialize)?;
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
