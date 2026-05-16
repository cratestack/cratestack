use cratestack_core::{Attribute, EnumVariant, Field, SourceSpan};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};
use crate::parse::types::parse_type_ref;

pub(super) fn parse_fields(lines: &[Line<'_>]) -> Result<Vec<Field>, SchemaError> {
    let mut fields = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        fields.push(parse_field(line, std::mem::take(&mut pending_docs))?);
    }
    Ok(fields)
}

pub(super) fn parse_enum_variants(lines: &[Line<'_>]) -> Result<Vec<EnumVariant>, SchemaError> {
    let mut variants = Vec::new();
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        if line.trimmed.is_empty() {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.starts_with("//") {
            pending_docs.clear();
            continue;
        }
        if line.trimmed.chars().any(char::is_whitespace) {
            return Err(SchemaError::new(
                "enum variants must be declared as a single identifier per line",
                line.start..line.start + line.raw.len(),
                line.number,
            ));
        }
        variants.push(EnumVariant {
            docs: std::mem::take(&mut pending_docs),
            name: line.trimmed.to_owned(),
            span: trimmed_span(line),
        });
    }
    Ok(variants)
}

pub(super) fn parse_field(line: &Line<'_>, docs: Vec<String>) -> Result<Field, SchemaError> {
    let mut parts = line.trimmed.splitn(3, char::is_whitespace);
    let name = parts.next().ok_or_else(|| {
        SchemaError::new(
            "expected field name",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let ty = parts.next().ok_or_else(|| {
        SchemaError::new(
            "expected field type",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let attrs = parts.next().unwrap_or_default();

    let trimmed_start = line.raw.find(line.trimmed).unwrap_or_default();
    let name_offset_in_trimmed = line.trimmed.find(name).unwrap_or_default();
    let after_name = &line.trimmed[name_offset_in_trimmed + name.len()..];
    let whitespace_after_name = after_name.len() - after_name.trim_start().len();
    let ty_offset_in_trimmed = name_offset_in_trimmed + name.len() + whitespace_after_name;
    let name_span = SourceSpan {
        start: line.start + trimmed_start + name_offset_in_trimmed,
        end: line.start + trimmed_start + name_offset_in_trimmed + name.len(),
        line: line.number,
    };
    let ty_span = SourceSpan {
        start: line.start + trimmed_start + ty_offset_in_trimmed,
        end: line.start + trimmed_start + ty_offset_in_trimmed + ty.len(),
        line: line.number,
    };
    let attrs_offset = if attrs.is_empty() {
        ty_span.end.saturating_sub(line.start)
    } else {
        line.raw
            .find(attrs)
            .unwrap_or(ty_span.end.saturating_sub(line.start))
    };
    let attribute_spans = split_field_attributes(attrs, attrs_offset);

    Ok(Field {
        docs,
        name: name.to_owned(),
        name_span,
        ty: parse_type_ref(ty, line, ty_span.start.saturating_sub(line.start))?,
        attributes: attribute_spans
            .into_iter()
            .map(|(raw, start, end)| Attribute {
                raw,
                span: SourceSpan {
                    start: line.start + start,
                    end: line.start + end,
                    line: line.number,
                },
            })
            .collect(),
        span: SourceSpan {
            start: line.start,
            end: line.start + line.raw.len(),
            line: line.number,
        },
    })
}

fn split_field_attributes(attrs: &str, offset: usize) -> Vec<(String, usize, usize)> {
    let mut attributes = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut current_start = None;

    for (index, ch) in attrs.char_indices() {
        if current.is_empty() {
            if ch == '@' {
                current.push(ch);
                current_start = Some(offset + index);
            }
            continue;
        }

        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ch if ch.is_whitespace() && depth == 0 => {
                let start = current_start.take().unwrap_or(offset + index);
                attributes.push((std::mem::take(&mut current), start, offset + index));
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        let start = current_start.unwrap_or(offset + attrs.len().saturating_sub(current.len()));
        attributes.push((current, start, offset + attrs.len()));
    }

    attributes
}
