use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrateError {
    #[error("failed to read snapshot file {path}: {source}")]
    SnapshotRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write snapshot file {path}: {source}")]
    SnapshotWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse snapshot file {path}: {source}")]
    SnapshotParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error(
        "snapshot file {path} has unsupported format version {found} \
         (this build understands version {expected}); regenerate it with a \
         compatible cratestack-migrate"
    )]
    SnapshotFormatVersion {
        path: PathBuf,
        found: u32,
        expected: u32,
    },

    #[error("failed to serialize snapshot: {0}")]
    SnapshotSerialize(#[source] serde_json::Error),
}
