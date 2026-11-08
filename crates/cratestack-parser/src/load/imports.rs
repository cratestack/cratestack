//! Loading `import`s depth first, in the pre-order of
//! docs/design/multi-file-schemas.md section 7: a unit's root, its parts,
//! then each of those files' imports in turn. A unit already loaded is
//! skipped (a diamond), one still being loaded is a cycle (M11).

use std::path::PathBuf;

use super::FileRole;
use super::graph::Graph;
use super::import_rules::{ImportEdge, check_import_file};
use super::paths::{check_directive_path, display_for, resolve, root_relative};
use super::sources::canonical_schema_path;
use crate::diagnostics::{SchemaError, span_error};
use crate::directives::PathDirective;

/// A unit being loaded: the main root, or an imported root.
pub(super) struct Frame {
    pub(super) path: PathBuf,
    pub(super) label: String,
    /// The part holding the `import` line that led to the next frame.
    pub(super) via: Option<String>,
}

impl Graph<'_> {
    /// Imports of every file in `files[start..end]`, which are one unit's.
    pub(super) fn load_imports(&mut self, start: usize, end: usize) -> Result<(), SchemaError> {
        for index in start..end {
            let imports = self.directives_of(index).imports;
            for import in &imports {
                self.load_import(index, import)?;
            }
        }
        Ok(())
    }

    fn load_import(&mut self, from: usize, import: &PathDirective) -> Result<(), SchemaError> {
        check_directive_path("import", import)?;
        let importer = self.files[from].file.relative.clone();
        let target = resolve(&self.files[from].abs, &import.path);
        let canonical = canonical_schema_path(&target);
        if self.done.contains(&canonical) {
            return Ok(());
        }
        let in_part = matches!(
            self.files[from].file.role,
            FileRole::Part | FileRole::ImportPart
        );
        let top = self.stack.len() - 1;
        self.stack[top].via = in_part.then(|| importer.clone());
        if let Some(position) = self.stack.iter().position(|f| f.path == canonical) {
            return Err(self.cycle(position, import));
        }
        let main = self.root_abs.clone();
        let relative = root_relative(&main, &target);
        let display = display_for(&self.root_display, &main, &target);
        self.note_attempt(&target);
        let text = self.sources.read(&target).map_err(|error| {
            span_error(
                format!(
                    "cannot read `{}` (listed on line {} of `{importer}`): {error}",
                    import.path, import.span.line
                ),
                import.span,
            )
        })?;
        let index = self.add_parsed(
            target.clone(),
            display,
            relative.clone(),
            FileRole::Import,
            text,
        )?;
        let directives = self.directives_of(index);
        if let Some(part_of) = &directives.part_of {
            let root = root_relative(&main, &resolve(&target, &part_of.path));
            return Err(span_error(
                format!(
                    "cannot import `{relative}`: it is a part of `{root}`, and a part is locked \
                     to its root; import a file that does not declare `part of` instead"
                ),
                import.span,
            )
            .with_related(
                format!("`{relative}` declares `part of` here"),
                part_of.span,
            ));
        }
        let edge = ImportEdge {
            importer,
            line: import.span,
        };
        check_import_file(&self.files[index], &edge)?;
        self.stack.push(Frame {
            path: canonical.clone(),
            label: relative,
            via: None,
        });
        self.load_parts(index, &directives, Some(&edge))?;
        let end = self.files.len();
        self.load_imports(index, end)?;
        self.stack.pop();
        self.done.insert(canonical);
        Ok(())
    }

    /// M11, from the main root down to the unit that closes the cycle.
    fn cycle(&self, closes: usize, import: &PathDirective) -> SchemaError {
        let mut chain: Vec<String> = self
            .stack
            .iter()
            .map(|frame| match &frame.via {
                Some(part) => format!("`{}` (part `{part}`)", frame.label),
                None => format!("`{}`", frame.label),
            })
            .collect();
        chain.push(format!("`{}`", self.stack[closes].label));
        span_error(format!("import cycle: {}", chain.join(" → ")), import.span)
    }
}
