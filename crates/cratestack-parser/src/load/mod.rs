//! Loading a schema from a file, with the `part`s it lists
//! (docs/design/multi-file-schemas.md section 14).

mod attribute;
mod collisions;
mod diagnostics;
mod file_io;
mod graph;
mod merge;
mod part_rules;
mod parts;
mod paths;
mod sources;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use cratestack_core::Schema;

use crate::diagnostics::SchemaError;

pub use self::diagnostics::load_schema_diagnostics;
pub use self::sources::{FsSources, OverlaySources, SchemaSources, canonical_schema_path};

/// How to load.
#[derive(Default, Clone, Copy)]
pub struct LoadOptions<'a> {
    /// Given a file that declares `part of`, load the root it names instead
    /// (`check` and the language server). Without it that is an error (M4).
    pub follow_part_of: bool,
    /// Where file text comes from. Default: the disk.
    pub sources: Option<&'a dyn SchemaSources>,
}

/// What a loaded file is to the schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRole {
    Root,
    Part,
    Import,
    ImportPart,
}

/// One file of a loaded schema.
#[derive(Debug, Clone)]
pub struct SchemaFile {
    /// The path as shown in a code frame.
    pub display: String,
    /// Relative to the root's directory, as shown in messages.
    pub relative: String,
    /// Canonical.
    pub path: PathBuf,
    pub source: Arc<str>,
    /// Offset of this file's first byte in the schema's address space.
    pub base: usize,
    pub role: FileRole,
}

/// A schema and the files it was loaded from, in load order (root first).
#[derive(Debug, Clone)]
pub struct LoadedSchema {
    pub schema: Schema,
    pub files: Vec<SchemaFile>,
}

impl LoadedSchema {
    pub fn root(&self) -> &SchemaFile {
        &self.files[0]
    }

    /// The file containing a global offset, and the offset within it.
    pub fn locate(&self, offset: usize) -> Option<(&SchemaFile, usize)> {
        self.files
            .iter()
            .find(|file| file.base <= offset && offset <= file.base + file.source.len())
            .map(|file| (file, offset - file.base))
    }

    pub fn is_single_file(&self) -> bool {
        self.files.len() == 1
    }
}

/// Everything a load found. `files` lists every file a read was tried for,
/// including when loading stopped early.
pub struct LoadDiagnostics {
    pub schema: Option<LoadedSchema>,
    pub errors: Vec<SchemaError>,
    pub files: Vec<PathBuf>,
}

/// Loads and validates, returning the first error. Same checks, in the same
/// order, as [`load_schema_diagnostics`].
pub fn load_schema(
    path: impl AsRef<Path>,
    options: &LoadOptions<'_>,
) -> Result<LoadedSchema, SchemaError> {
    let diagnostics = load_schema_diagnostics(path, options);
    match diagnostics.schema {
        Some(loaded) => Ok(loaded),
        None => Err(diagnostics
            .errors
            .into_iter()
            .next()
            .expect("a load without a schema reports an error")),
    }
}
