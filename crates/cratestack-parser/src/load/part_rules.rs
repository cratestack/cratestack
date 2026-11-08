//! What a listed part must satisfy: M1, M2, M5, M6
//! (docs/design/multi-file-schemas.md sections 4 and 9).

use std::path::Path;

use super::graph::LoadedFile;
use super::paths::{check_directive_path, lexical_relative, resolve, root_relative};
use super::sources::canonical_schema_path;
use crate::diagnostics::{SchemaError, span_error};
use crate::directives::PathDirective;

/// `part` was read because the root's `listing` line named it.
pub(super) fn check_part(
    root_abs: &Path,
    part: &LoadedFile,
    listing: &PathDirective,
) -> Result<(), SchemaError> {
    let parsed = part
        .parsed
        .as_ref()
        .expect("a part is parsed before it is checked");
    let directives = &parsed.directives;
    let root = root_relative(root_abs, root_abs);
    let name = &part.file.relative;
    match &directives.part_of {
        None => {
            let back = lexical_relative(part.abs.parent().unwrap_or(Path::new("")), root_abs);
            return Err(span_error(
                format!(
                    "`{name}` is listed as a part of `{root}`, but does not declare `part of`; \
                     add `part of \"{back}\"` to `{name}`"
                ),
                listing.span,
            ));
        }
        Some(part_of) => {
            check_directive_path("part of", part_of)?;
            let target = resolve(&part.abs, &part_of.path);
            if canonical_schema_path(&target) != canonical_schema_path(root_abs) {
                let other = root_relative(root_abs, &target);
                return Err(span_error(
                    format!(
                        "`{name}` is listed as a part of `{root}`, but declares itself a part of \
                         `{other}`"
                    ),
                    part_of.span,
                )
                .with_related(format!("`{name}` is listed here"), listing.span));
            }
        }
    }
    let singletons = [
        (
            "datasource",
            parsed.schema.datasource.as_ref().map(|d| d.span),
        ),
        ("auth", parsed.schema.auth.as_ref().map(|a| a.span)),
        ("transport", directives.transport_span()),
        ("mcp", parsed.schema.mcp.as_ref().map(|m| m.span)),
    ];
    let first = singletons
        .into_iter()
        .filter_map(|(kind, span)| span.map(|span| (kind, span)))
        .min_by_key(|(_, span)| span.start);
    if let Some((kind, span)) = first {
        return Err(span_error(
            format!("`{kind}` belongs in the root schema (`{root}`), not in a part"),
            span,
        ));
    }
    if let Some(nested) = directives.parts.first() {
        return Err(span_error(
            format!(
                "a part cannot list parts of its own; move `part \"{}\"` to the root schema \
                 (`{root}`)",
                nested.path
            ),
            nested.span,
        ));
    }
    Ok(())
}
