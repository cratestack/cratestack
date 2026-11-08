//! Loading the parts a unit's root lists (the main root, or an imported one),
//! with the checks that run before each is read (M8, M9, M14, M15).

use std::path::PathBuf;

use super::FileRole;
use super::graph::Graph;
use super::import_rules::{ImportEdge, check_import_file};
use super::part_rules::check_part;
use super::paths::{check_directive_path, display_for, resolve, root_relative};
use super::sources::canonical_schema_path;
use crate::diagnostics::{SchemaError, span_error};
use crate::directives::FileDirectives;

impl Graph<'_> {
    /// `unit` is the index of the unit's root in `files`; `edge` is how the
    /// unit was imported, or `None` for the main root.
    pub(super) fn load_parts(
        &mut self,
        unit: usize,
        directives: &FileDirectives,
        edge: Option<&ImportEdge>,
    ) -> Result<(), SchemaError> {
        let unit_abs = self.files[unit].abs.clone();
        let unit_path = self.files[unit].file.path.clone();
        let main = self.root_abs.clone();
        let root = root_relative(&main, &unit_abs);
        let role = if edge.is_some() {
            FileRole::ImportPart
        } else {
            FileRole::Part
        };
        let mut seen: Vec<(PathBuf, usize)> = Vec::new();
        for listing in &directives.parts {
            check_directive_path("part", listing)?;
            let abs = resolve(&unit_abs, &listing.path);
            let canonical = canonical_schema_path(&abs);
            if canonical == unit_path {
                return Err(span_error(
                    format!("`{root}` lists itself as a part"),
                    listing.span,
                ));
            }
            if let Some((_, line)) = seen.iter().find(|(path, _)| *path == canonical) {
                return Err(span_error(
                    format!(
                        "`{}` is already listed as a part on line {line}",
                        listing.path
                    ),
                    listing.span,
                ));
            }
            seen.push((canonical, listing.span.line));
            let relative = root_relative(&main, &abs);
            let display = display_for(&self.root_display, &main, &abs);
            self.note_attempt(&abs);
            let text = self.sources.read(&abs).map_err(|error| {
                span_error(
                    format!(
                        "cannot read `{}` (listed on line {} of `{root}`): {error}",
                        listing.path, listing.span.line
                    ),
                    listing.span,
                )
            })?;
            let index = self.add_parsed(abs, display, relative, role, text)?;
            if let Some(edge) = edge {
                check_import_file(&self.files[index], edge)?;
            }
            check_part(&main, &unit_abs, &self.files[index], listing)?;
        }
        Ok(())
    }
}
