use cratestack_core::schema::attribute_text::strip_comment;
use cratestack_core::{Attribute, EnumVariant, Field, SourceSpan};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};
use crate::parse::attribute_spacing::split_field_attributes;
use crate::parse::format_chars::refuse_invisible_characters;
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

/// Length of the type token at the start of `rest`, treating whitespace
/// inside a parenthesized argument list as part of the type rather than
/// a token boundary.
///
/// `String` and `Vector(1536)` are unaffected (no spaces to protect);
/// `Geography(Polygon, 4326)` is the case this exists for. An unclosed
/// paren consumes to end-of-line, which then fails in `parse_type_ref`
/// with the usual "invalid type reference" diagnostic rather than
/// silently truncating at the space.
fn type_token_len(rest: &str) -> usize {
    let mut depth = 0usize;
    for (index, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ if ch.is_whitespace() && depth == 0 => return index,
            _ => {}
        }
    }
    rest.len()
}

pub(super) fn parse_field(line: &Line<'_>, docs: Vec<String>) -> Result<Field, SchemaError> {
    // A trailing `//` comment is not part of the field: without this,
    // `name String @unique // not @readonly` made `@readonly` live
    // (GHSA-69g4-xvcm-vm2j). Quote-aware, so `"http://…"` is untouched.
    let content = strip_comment(line.trimmed);
    let mut parts = content.splitn(2, char::is_whitespace);
    let name = parts.next().ok_or_else(|| {
        SchemaError::new(
            "expected field name",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    let trimmed_start = line.raw.find(line.trimmed).unwrap_or_default();
    let name_offset_in_trimmed = content.find(name).unwrap_or_default();
    let after_name = &content[name_offset_in_trimmed + name.len()..];
    let whitespace_after_name = after_name.len() - after_name.trim_start().len();
    let ty_offset_in_trimmed = name_offset_in_trimmed + name.len() + whitespace_after_name;

    // The type is *not* simply the next whitespace-delimited token: a
    // parametric scalar's argument list may contain spaces, as in
    // `Geography(Polygon, 4326)` (cratestack#842). Scan to the first
    // whitespace at paren-depth zero so the whole type — arguments
    // included — stays together, and whatever follows is attributes.
    let rest = &content[ty_offset_in_trimmed..];
    let ty_len = type_token_len(rest);
    let ty = &rest[..ty_len];
    if ty.is_empty() {
        return Err(SchemaError::new(
            "expected field type",
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    }
    let attrs = rest[ty_len..].trim_start();
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
    let attributes = split_field_attributes(attrs, attrs_offset, name, line)?
        .into_iter()
        .map(|(raw, start, end)| Attribute {
            raw,
            span: SourceSpan {
                start: line.start + start,
                end: line.start + end,
                line: line.number,
            },
        })
        .collect::<Vec<_>>();
    refuse_invisible_characters(&attributes, std::slice::from_ref(line))?;

    Ok(Field {
        docs,
        name: name.to_owned(),
        name_span,
        ty: parse_type_ref(ty, line, ty_span.start.saturating_sub(line.start))?,
        attributes,
        span: SourceSpan {
            start: line.start,
            end: line.start + line.raw.len(),
            line: line.number,
        },
    })
}
