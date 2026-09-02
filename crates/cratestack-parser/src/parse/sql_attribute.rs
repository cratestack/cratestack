//! Multi-line `@@…_sql("""…""")` attribute capture, shared by the `view`
//! and `query` block parsers.
//!
//! Extracted from `parse/views.rs` when `query` (cratestack#867) arrived
//! needing byte-identical behaviour: both constructs let one attribute's
//! value span several physical lines, and both must preserve the SQL
//! verbatim in [`Attribute::raw`](cratestack_core::Attribute::raw) so the
//! author's own formatting survives into the generated `const SQL`. A
//! second copy would have been free to drift on exactly the details that
//! are invisible until someone's `FILTER (WHERE …)` clause loses a
//! newline.

use cratestack_core::SourceSpan;

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, trimmed_span};

/// Attribute names whose value may span multiple physical lines.
///
/// `view` uses all three (`@@server_sql`/`@@embedded_sql` for the
/// per-backend split, `@@sql` as the shorthand); `query` uses only
/// `@@sql` and rejects the other two in its semantic pass, because it is
/// Postgres-only by design and an `@@embedded_sql` on it would promise a
/// backend that does not exist (design §4).
pub(super) const SQL_ATTRS: &[&str] = &["@@server_sql", "@@embedded_sql", "@@sql"];

/// Collect one `@@…` attribute starting at `lines[start]`, stitching
/// continuation lines together when it opens a `"""` body that the same
/// line does not close.
///
/// `construct` names the enclosing block (`"view"`, `"query"`) and is used
/// only in the unterminated-body error, so the message points the author
/// at the block they are actually editing.
pub(super) fn collect_attribute_text(
    lines: &[Line<'_>],
    start: usize,
    construct: &str,
) -> Result<(String, SourceSpan, usize), SchemaError> {
    let first = &lines[start];
    let trimmed = first.trimmed;

    // Only the SQL-body attributes support multi-line capture. Any other
    // `@@…` attribute is a single line.
    let opens_multiline_sql = SQL_ATTRS.iter().any(|prefix| trimmed.starts_with(prefix))
        && trimmed.contains("(\"\"\"")
        && !single_line_triple_closed(trimmed);

    if !opens_multiline_sql {
        return Ok((trimmed.to_owned(), trimmed_span(first), start + 1));
    }

    let mut buffer = first.raw.to_owned();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = &lines[cursor];
        buffer.push('\n');
        buffer.push_str(line.raw);
        if line.raw.contains("\"\"\")") {
            let span = SourceSpan {
                start: first.start + leading_ws(first.raw),
                end: line.start + line.raw.len(),
                line: first.number,
            };
            return Ok((buffer.trim().to_owned(), span, cursor + 1));
        }
        cursor += 1;
    }

    Err(SchemaError::new(
        format!("unterminated `\"\"\"` SQL body in {construct} attribute"),
        first.start..first.start + first.raw.len(),
        first.number,
    ))
}

fn single_line_triple_closed(trimmed: &str) -> bool {
    // Check if the same physical line both opens and closes a triple-
    // quoted body, in which case no multi-line stitching is needed.
    let after_open = match trimmed.split_once("(\"\"\"") {
        Some((_, rest)) => rest,
        None => return false,
    };
    after_open.contains("\"\"\"")
}

fn leading_ws(raw: &str) -> usize {
    raw.bytes()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count()
}
