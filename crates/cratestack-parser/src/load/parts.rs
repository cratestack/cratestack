//! Loading the parts a root lists, with the checks that run before each is
//! read (M8, M9, M14, M15).

use std::path::PathBuf;

use super::FileRole;
use super::graph::Graph;
use super::part_rules::check_part;
use super::paths::{check_directive_path, display_for, resolve, root_relative};
use super::sources::canonical_schema_path;
use crate::diagnostics::{SchemaError, span_error};
use crate::directives::FileDirectives;

impl Graph<'_> {
    pub(super) fn load_parts(&mut self, directives: &FileDirectives) -> Result<(), SchemaError> {
        let root_abs = self.root_abs.clone();
        let root = root_relative(&root_abs, &root_abs);
        let mut seen: Vec<(PathBuf, usize)> = Vec::new();
        for listing in &directives.parts {
            check_directive_path("part", listing)?;
            let abs = resolve(&root_abs, &listing.path);
            let canonical = canonical_schema_path(&abs);
            if canonical == self.files[0].file.path {
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
            let relative = root_relative(&root_abs, &abs);
            let display = display_for(&self.root_display, &root_abs, &abs);
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
            let index = self.add_parsed(abs, display, relative, FileRole::Part, text)?;
            check_part(&root_abs, &self.files[index], listing)?;
        }
        Ok(())
    }
}
