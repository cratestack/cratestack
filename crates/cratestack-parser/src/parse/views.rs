//! `view <Name> from <Model>, <Model>, ... { ... }` block parser.
//!
//! See ADR-0003 (`internals/views-adr.md` in cratestack-docs).
//!
//! The view body parses the same as a model body — `parse_field` for
//! field lines, `@@…` lines collected as block-level attributes — with
//! one extra capability: `@@server_sql` / `@@embedded_sql` / `@@sql`
//! values are allowed to span multiple physical lines using triple-
//! quoted strings (`"""…"""`). The continuation logic lives here so
//! the SQL body is captured verbatim in the `Attribute.raw` field.

use cratestack_core::{Attribute, Field, SourceSpan, View, ViewSource};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};
use crate::parse::fields::parse_field;

const SQL_ATTRS: &[&str] = &["@@server_sql", "@@embedded_sql", "@@sql"];

pub(super) fn parse_view_block<'a>(
    lines: &'a [Line<'a>],
    start: usize,
    docs: Vec<String>,
) -> Result<(View, usize), SchemaError> {
    let header = &lines[start];
    let (name, name_span, sources) = parse_view_header(header)?;

    let mut body = Vec::new();
    let mut cursor = start + 1;
    while cursor < lines.len() {
        let line = &lines[cursor];
        if line.trimmed == "}" {
            let span = SourceSpan {
                start: header.start,
                end: line.start + line.raw.len(),
                line: header.number,
            };
            let (fields, attributes) = parse_view_body(&body)?;
            return Ok((
                View {
                    docs,
                    name,
                    name_span,
                    sources,
                    fields,
                    attributes,
                    span,
                },
                cursor + 1,
            ));
        }
        body.push(line.clone());
        cursor += 1;
    }

    Err(SchemaError::new(
        "unterminated view block".to_owned(),
        header.start..header.start + header.raw.len(),
        header.number,
    ))
}

fn parse_view_header(
    header: &Line<'_>,
) -> Result<(String, SourceSpan, Vec<ViewSource>), SchemaError> {
    let trimmed = header.trimmed;
    let after_keyword = trimmed.strip_prefix("view ").ok_or_else(|| {
        SchemaError::new(
            "expected `view` declaration".to_owned(),
            header.start..header.start + header.raw.len(),
            header.number,
        )
    })?;
    let header_body = after_keyword
        .strip_suffix('{')
        .map(str::trim)
        .ok_or_else(|| {
            SchemaError::new(
                "view block header must end with `{`".to_owned(),
                header.start..header.start + header.raw.len(),
                header.number,
            )
        })?;

    // Split on ` from ` (with surrounding whitespace). If absent, the
    // view has no declared source models — the validator will error,
    // but the parser still produces a parseable shape.
    let (name_part, sources_part) = match header_body.split_once(" from ") {
        Some(pair) => pair,
        None => (header_body, ""),
    };
    let name = name_part.trim().to_owned();
    if name.is_empty() {
        return Err(SchemaError::new(
            "view block missing name".to_owned(),
            header.start..header.start + header.raw.len(),
            header.number,
        ));
    }
    let name_span = span_of_substring(header, &name).unwrap_or_else(|| trimmed_span(header));

    let mut sources = Vec::new();
    for raw_source in sources_part.split(',') {
        let trimmed_source = raw_source.trim();
        if trimmed_source.is_empty() {
            continue;
        }
        let source_span =
            span_of_substring(header, trimmed_source).unwrap_or_else(|| trimmed_span(header));
        sources.push(ViewSource {
            name: trimmed_source.to_owned(),
            name_span: source_span,
        });
    }

    Ok((name, name_span, sources))
}

fn parse_view_body(lines: &[Line<'_>]) -> Result<(Vec<Field>, Vec<Attribute>), SchemaError> {
    let mut fields = Vec::new();
    let mut attributes = Vec::new();
    let mut pending_docs = Vec::new();
    let mut cursor = 0usize;

    while cursor < lines.len() {
        let line = &lines[cursor];
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            cursor += 1;
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            cursor += 1;
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            cursor += 1;
            continue;
        }
        if line.trimmed.starts_with("@@") {
            pending_docs.clear();
            // Multi-line capture for `@@…_sql("""…""")` — extend the
            // attribute text until the matching closing triple quote.
            let (raw, span, next) = collect_attribute_text(lines, cursor)?;
            attributes.push(Attribute { raw, span });
            cursor = next;
            continue;
        }
        if line.trimmed.starts_with('@') {
            return Err(SchemaError::new(
                format!("unsupported view directive `{}`", line.trimmed),
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        }
        fields.push(parse_field(line, std::mem::take(&mut pending_docs))?);
        cursor += 1;
    }
    Ok((fields, attributes))
}

fn collect_attribute_text(
    lines: &[Line<'_>],
    start: usize,
) -> Result<(String, SourceSpan, usize), SchemaError> {
    let first = &lines[start];
    let trimmed = first.trimmed;

    // Only `@@server_sql` / `@@embedded_sql` / `@@sql` support multi-
    // line capture. Any other `@@…` attribute is a single line.
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
        "unterminated `\"\"\"` SQL body in view attribute".to_owned(),
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

fn span_of_substring(line: &Line<'_>, needle: &str) -> Option<SourceSpan> {
    let raw = line.raw;
    let offset = raw.find(needle)?;
    Some(SourceSpan {
        start: line.start + offset,
        end: line.start + offset + needle.len(),
        line: line.number,
    })
}

fn leading_ws(raw: &str) -> usize {
    raw.bytes()
        .take_while(|byte| byte.is_ascii_whitespace())
        .count()
}
