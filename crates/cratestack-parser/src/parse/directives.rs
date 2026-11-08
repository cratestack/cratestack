//! Parsing of one `part` / `part of` / `import` line (M16).

use cratestack_core::SourceSpan;

use crate::diagnostics::SchemaError;
use crate::directives::PathDirective;
use crate::line_helpers::{Line, trimmed_span};

/// `keyword` is `"part of"`, `"part"` or `"import"`. Paths are stored as
/// written; whether they are acceptable paths is the loader's concern.
pub(super) fn parse_path_directive(
    line: &Line<'_>,
    keyword: &str,
) -> Result<PathDirective, SchemaError> {
    let span = trimmed_span(line);
    let malformed = || {
        SchemaError::new(
            format!(
                "expected a quoted path after `{keyword}`, like `{keyword} \"models/post.cstack\"`"
            ),
            span.start..span.end,
            line.number,
        )
    };
    let rest = line.trimmed.strip_prefix(keyword).ok_or_else(malformed)?;
    // `part` must not match the front of another word (`partial`).
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return Err(malformed());
    }
    let after = rest.trim_start();
    let inner = after.strip_prefix('"').ok_or_else(malformed)?;
    let close = inner.find('"').ok_or_else(malformed)?;
    if !inner[close + 1..].is_empty() {
        return Err(malformed());
    }
    // `after` is a suffix of `line.trimmed`, itself located in `span`.
    let quote_offset = line.trimmed.len() - after.len() + 1;
    let path_start = span.start + quote_offset;
    Ok(PathDirective {
        path: inner[..close].to_owned(),
        path_span: SourceSpan {
            start: path_start,
            end: path_start + close,
            line: line.number,
        },
        span,
    })
}
