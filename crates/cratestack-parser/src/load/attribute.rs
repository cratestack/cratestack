//! Turns an error in the schema's one address space into an error in one
//! file (docs/design/multi-file-schemas.md section 9).

use std::ops::Range;
use std::sync::Arc;

use super::SchemaFile;
use crate::diagnostics::SchemaError;

/// Picks the file whose `[base, base + len]` contains each span's start (the
/// root when none does), rebases the span to that file and attaches the
/// file's display path and source. An error that already names a file (the
/// root could not be read) is left alone.
pub(super) fn attribute<'a>(
    mut error: SchemaError,
    files: impl Iterator<Item = &'a SchemaFile> + Clone,
) -> SchemaError {
    if !error.file.is_empty() {
        return error;
    }
    let Some(first) = files.clone().next() else {
        return error;
    };
    let owner = |offset: usize| {
        files
            .clone()
            .find(|file| file.base <= offset && offset <= file.base + file.source.len())
            .unwrap_or(first)
    };
    let rebase = |span: &Range<usize>, file: &SchemaFile| {
        span.start.saturating_sub(file.base)..span.end.saturating_sub(file.base)
    };
    let file = owner(error.span.start);
    error.span = rebase(&error.span, file);
    error.file = Arc::from(file.display.as_str());
    error.source_text = Arc::clone(&file.source);
    for related in error.related.iter_mut().filter(|r| r.file.is_empty()) {
        let file = owner(related.span.start);
        related.span = rebase(&related.span, file);
        related.file = Arc::from(file.display.as_str());
        related.source_text = Arc::clone(&file.source);
    }
    error
}
