//! Reading one file into the graph at the next base.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::graph::{Graph, LoadedFile};
use super::sources::canonical_schema_path;
use super::{FileRole, SchemaFile};
use crate::diagnostics::SchemaError;
use crate::directives::FileDirectives;
use crate::parse::parse_file_at;

impl Graph<'_> {
    pub(super) fn read_and_parse(
        &mut self,
        abs: &Path,
        display: String,
        relative: String,
        role: FileRole,
    ) -> Result<usize, SchemaError> {
        self.note_attempt(abs);
        let text = self.sources.read(abs).map_err(|error| {
            SchemaError::new(
                format!("failed to read schema file {display}: {error}"),
                0..0,
                1,
            )
            .with_file(&Arc::from(display.as_str()), &Arc::from(""))
        })?;
        self.add_parsed(abs.to_path_buf(), display, relative, role, text)
    }

    /// Appends the file at the next base and parses it there. A parse error
    /// is returned in the schema's address space, with the file already
    /// listed so it can be attributed to it.
    pub(super) fn add_parsed(
        &mut self,
        abs: PathBuf,
        display: String,
        relative: String,
        role: FileRole,
        text: String,
    ) -> Result<usize, SchemaError> {
        let base = self
            .files
            .last()
            .map_or(0, |last| last.file.base + last.file.source.len() + 1);
        let source: Arc<str> = Arc::from(text);
        let result = parse_file_at(&source, base);
        let (parsed, error) = match result {
            Ok(parsed) => (Some(parsed), None),
            Err(error) => (None, Some(error)),
        };
        self.files.push(LoadedFile {
            file: SchemaFile {
                display,
                relative,
                path: canonical_schema_path(&abs),
                source,
                base,
                role,
            },
            abs,
            parsed,
        });
        match error {
            Some(error) => Err(error),
            None => Ok(self.files.len() - 1),
        }
    }

    pub(super) fn directives_of(&self, index: usize) -> FileDirectives {
        self.files[index]
            .parsed
            .as_ref()
            .map(|p| p.directives.clone())
            .unwrap_or_default()
    }

    pub(super) fn note_attempt(&mut self, abs: &Path) {
        let canonical = canonical_schema_path(abs);
        if !self.attempted.contains(&canonical) {
            self.attempted.push(canonical);
        }
    }
}
