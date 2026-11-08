//! Reading and parsing the files of one schema in load order
//! (docs/design/multi-file-schemas.md sections 7 and 9): a root, its parts,
//! then its imports (`imports.rs`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::attribute::attribute;
use super::imports::Frame;
use super::paths::{check_directive_path, display_for, resolve, root_relative};
use super::sources::{SchemaSources, canonical_schema_path};
use super::{FileRole, SchemaFile};
use crate::diagnostics::{SchemaError, span_error};
use crate::directives::{FileDirectives, PathDirective};
use crate::parse::ParsedFile;

/// One file read: its identity, and its parse (absent only when it failed to
/// parse, so that the failure can still be attributed to it).
pub(super) struct LoadedFile {
    pub(super) file: SchemaFile,
    /// Lexical absolute path: what directive paths resolve against.
    pub(super) abs: PathBuf,
    pub(super) parsed: Option<ParsedFile>,
}

pub(super) struct Graph<'a> {
    pub(super) files: Vec<LoadedFile>,
    /// Every file a read was tried for, in order, canonical.
    pub(super) attempted: Vec<PathBuf>,
    pub(super) sources: &'a dyn SchemaSources,
    pub(super) root_abs: PathBuf,
    pub(super) root_display: String,
    /// The units being loaded, main root first (cycle detection).
    pub(super) stack: Vec<Frame>,
    /// Canonical paths of imported units fully loaded (diamonds).
    pub(super) done: HashSet<PathBuf>,
}

impl<'a> Graph<'a> {
    pub(super) fn new(sources: &'a dyn SchemaSources) -> Self {
        Self {
            files: Vec::new(),
            attempted: Vec::new(),
            sources,
            root_abs: PathBuf::new(),
            root_display: String::new(),
            stack: Vec::new(),
            done: HashSet::new(),
        }
    }

    /// Loads `path` and what it lists. With `follow`, a file that declares
    /// `part of` starts the load from the root it names instead.
    pub(super) fn load(&mut self, path: &Path, follow: bool) -> Result<(), SchemaError> {
        self.load_unattributed(path, follow)
            .map_err(|error| attribute(error, self.files.iter().map(|f| &f.file)))
    }

    fn load_unattributed(&mut self, path: &Path, follow: bool) -> Result<(), SchemaError> {
        let given_abs = canonical_schema_path(path);
        let given_display = path.display().to_string();
        let directives = self.load_root_file(given_abs.clone(), given_display.clone())?;
        let Some(part_of) = directives.part_of.clone() else {
            return self.load_parts_and_check(&directives);
        };
        check_directive_path("part of", &part_of)?;
        if !follow {
            return Err(self.part_given_as_root(&part_of));
        }
        let target = resolve(&given_abs, &part_of.path);
        let display = display_for(&given_display, &given_abs, &target);
        self.files.clear();
        let directives = self.load_root_file(canonical_schema_path(&target), display)?;
        if let Some(again) = &directives.part_of {
            return Err(self.part_given_as_root(again));
        }
        self.load_parts_and_check(&directives)?;
        self.require_listed(&given_abs)
    }

    fn load_parts_and_check(&mut self, directives: &FileDirectives) -> Result<(), SchemaError> {
        self.stack = vec![Frame {
            path: self.files[0].file.path.clone(),
            label: root_relative(&self.root_abs, &self.root_abs),
            via: None,
        }];
        self.done.clear();
        self.load_parts(0, directives, None)?;
        let end = self.files.len();
        self.load_imports(0, end)
    }

    /// M4: the file loaded as a root says it is a part.
    fn part_given_as_root(&self, part_of: &PathDirective) -> SchemaError {
        let file = &self.files[0].file.relative;
        span_error(
            format!(
                "`{file}` is a part of `{0}`, not a schema root; point the macro or command at \
                 `{0}`",
                part_of.path
            ),
            part_of.span,
        )
    }

    /// M3: followed a `part of` to a root that does not list the given file.
    fn require_listed(&mut self, given_abs: &Path) -> Result<(), SchemaError> {
        let listed = self.files.iter().skip(1).any(|f| f.file.path == *given_abs);
        if listed {
            return Ok(());
        }
        let relative = root_relative(&self.root_abs, given_abs);
        let display = display_for(&self.root_display, &self.root_abs, given_abs);
        let index = self.read_and_parse(given_abs, display, relative.clone(), FileRole::Part)?;
        let part_of = self.files[index]
            .parsed
            .as_ref()
            .and_then(|p| p.directives.part_of.clone())
            .expect("the file declared `part of` when it was followed");
        let root = root_relative(&self.root_abs, &self.root_abs);
        Err(span_error(
            format!(
                "`{relative}` declares itself a part of `{root}`, but `{root}` does not list it; \
                 add `part \"{relative}\"` to `{root}`"
            ),
            part_of.span,
        ))
    }

    fn load_root_file(
        &mut self,
        abs: PathBuf,
        display: String,
    ) -> Result<FileDirectives, SchemaError> {
        self.root_abs = abs.clone();
        self.root_display = display.clone();
        self.note_attempt(&abs);
        let text = self.sources.read(&abs).map_err(|error| {
            SchemaError::new(
                format!("failed to read schema file {display}: {error}"),
                0..0,
                1,
            )
            .with_file(&Arc::from(display.as_str()), &Arc::from(""))
        })?;
        let relative = root_relative(&abs, &abs);
        let index = self.add_parsed(abs, display, relative, FileRole::Root, text)?;
        Ok(self.directives_of(index))
    }
}
