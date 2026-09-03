//! `view <Name> from <Model>, <Model>, ... { ... }` block parser.
//!
//! See ADR-0003 (`internals/views-adr.md` in cratestack-docs).
//!
//! The view body parses the same as a model body — `parse_field` for
//! field lines, `@@…` lines collected as block-level attributes — with
//! one extra capability: `@@server_sql` / `@@embedded_sql` / `@@sql`
//! values are allowed to span multiple physical lines using triple-
//! quoted strings (`"""…"""`). That continuation logic lives in
//! [`crate::parse::sql_attribute`], shared with the `query` block parser
//! (cratestack#867), so the SQL body is captured verbatim in the
//! `Attribute.raw` field the same way for both constructs.

use cratestack_core::{Attribute, Field, SourceSpan, View, ViewSource};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};
use crate::parse::fields::parse_field;
use crate::parse::sql_attribute::collect_attribute_text;

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
            let (raw, span, next) = collect_attribute_text(lines, cursor, "view")?;
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

fn span_of_substring(line: &Line<'_>, needle: &str) -> Option<SourceSpan> {
    let raw = line.raw;
    let offset = raw.find(needle)?;
    Some(SourceSpan {
        start: line.start + offset,
        end: line.start + offset + needle.len(),
        line: line.number,
    })
}
