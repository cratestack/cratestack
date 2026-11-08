//! Parsing of one `part` / `part of` / `import` line (M16).

use cratestack_core::SourceSpan;

use crate::diagnostics::SchemaError;
use crate::directives::{FileDirectives, PathDirective};
use crate::line_helpers::{Line, trimmed_span};

/// `keyword` is `"part of"`, `"part"` or `"import"`. Paths are stored as
/// written; whether they are acceptable paths is the loader's concern.
pub(crate) fn parse_path_directive(
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

/// If `line` is a `part`, `part of` or `import` directive, record it in
/// `directives` and return `true`. A second `part of` is M7.
pub(super) fn record_directive(
    line: &Line<'_>,
    directives: &mut FileDirectives,
) -> Result<bool, SchemaError> {
    let Some(keyword) = directive_keyword(line.trimmed) else {
        return Ok(false);
    };
    let directive = parse_path_directive(line, keyword)?;
    match keyword {
        "part of" => {
            if let Some(first) = &directives.part_of {
                return Err(SchemaError::new(
                    format!(
                        "duplicate `part of` (first declared on line {})",
                        first.span.line
                    ),
                    directive.span.start..directive.span.end,
                    line.number,
                ));
            }
            directives.part_of = Some(directive);
        }
        "part" => directives.parts.push(directive),
        _ => directives.imports.push(directive),
    }
    Ok(true)
}

/// `part of` is matched before `part`, and the bare words too, so a missing
/// path reports M16 rather than an unsupported declaration.
pub(crate) fn directive_keyword(trimmed: &str) -> Option<&'static str> {
    let is = |keyword: &str| {
        trimmed == keyword
            || trimmed
                .strip_prefix(keyword)
                .is_some_and(|rest| rest.starts_with(char::is_whitespace))
    };
    ["part of", "part", "import"]
        .into_iter()
        .find(|keyword| is(keyword))
}
