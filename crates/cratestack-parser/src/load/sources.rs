//! Where the loader gets file text from: the disk, or an editor's unsaved
//! buffers laid over it (cratestack-lsp).

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use super::paths::fold;

/// Where file text comes from.
pub trait SchemaSources {
    fn read(&self, path: &Path) -> io::Result<String>;
}

/// The default: `std::fs::read_to_string`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsSources;

impl SchemaSources for FsSources {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }
}

/// In-memory texts keyed by [`canonical_schema_path`], optionally falling
/// back to the disk for a path that has none.
#[derive(Debug, Clone)]
pub struct OverlaySources {
    files: HashMap<PathBuf, String>,
    fallback_to_disk: bool,
}

impl OverlaySources {
    pub fn new(fallback_to_disk: bool) -> Self {
        Self {
            files: HashMap::new(),
            fallback_to_disk,
        }
    }

    pub fn insert(&mut self, path: impl AsRef<Path>, text: impl Into<String>) {
        self.files
            .insert(canonical_schema_path(path.as_ref()), text.into());
    }
}

impl SchemaSources for OverlaySources {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.files.get(&canonical_schema_path(path)) {
            Some(text) => Ok(text.clone()),
            None if self.fallback_to_disk => std::fs::read_to_string(path),
            None => Err(io::Error::from(io::ErrorKind::NotFound)),
        }
    }
}

/// The identity of a schema file, so two spellings of one file are one file
/// (docs/design/multi-file-schemas.md section 5, Q3): `std::fs::canonicalize`,
/// or, for a file that does not exist, the absolute path with `.` and `..`
/// folded away lexically.
pub fn canonical_schema_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    fold(&absolute)
}
