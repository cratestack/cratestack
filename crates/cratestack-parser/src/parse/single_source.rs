use cratestack_core::Schema;

use crate::diagnostics::{SchemaError, span_error};

use super::{ParsedFile, expand_mixins, parse_file_at};

/// Parse one self-contained source. A multi-file directive needs the file it
/// is written in to resolve its path against, so every single-source entry
/// point refuses one (M17) instead of ignoring it.
pub(crate) fn parse_schema_only(source: &str) -> Result<Schema, SchemaError> {
    let ParsedFile {
        mut schema,
        directives,
    } = parse_file_at(source, 0)?;
    let first = directives
        .parts
        .iter()
        .map(|d| ("part", d))
        .chain(directives.part_of.iter().map(|d| ("part of", d)))
        .chain(directives.imports.iter().map(|d| ("import", d)))
        .min_by_key(|(_, d)| d.span.start);
    if let Some((keyword, directive)) = first {
        return Err(span_error(
            format!(
                "`{keyword}` needs the schema to be loaded from a file, because \"{}\" is \
                 resolved against that file's directory; use `parse_schema_file` or \
                 `cratestack check --schema <root>`",
                directive.path
            ),
            directive.span,
        ));
    }
    expand_mixins(&mut schema)?;
    Ok(schema)
}
