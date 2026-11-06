//! An `enum { }` body: one variant per line, optionally numbered
//! (`Pending = 1`), and at most one `@@reserved(n, …)` line retiring
//! ordinals (`docs/design/enum-ordinals.md`). Range, uniqueness and
//! all-or-none are the validator's (`validate::enum_ordinals`); this file
//! only reads the shapes.

use cratestack_core::schema::attribute_text::strip_comment;
use cratestack_core::{ENUM_ORDINAL_MAX, ENUM_RESERVED_ATTRIBUTE, EnumVariant, ReservedOrdinals};

use crate::diagnostics::{SchemaError, span_error};
use crate::line_helpers::{Line, parse_doc_comment, trimmed_span};

pub(super) struct EnumBody {
    pub(super) variants: Vec<EnumVariant>,
    pub(super) reserved: Option<ReservedOrdinals>,
}

pub(super) fn parse_enum_body(lines: &[Line<'_>]) -> Result<EnumBody, SchemaError> {
    let mut body = EnumBody {
        variants: Vec::new(),
        reserved: None,
    };
    let mut pending_docs = Vec::new();
    for line in lines {
        if let Some(doc) = parse_doc_comment(line) {
            pending_docs.push(doc.to_owned());
            continue;
        }
        let text = strip_comment(line.trimmed).trim_end();
        if text.is_empty() {
            pending_docs.clear();
            continue;
        }
        let span = trimmed_span(line);
        let span = cratestack_core::SourceSpan {
            end: span.start + text.len(),
            ..span
        };
        if text.starts_with('@') {
            pending_docs.clear();
            let reserved = parse_reserved(text, span)?;
            if body.reserved.is_some() {
                return Err(span_error(
                    format!(
                        "an enum takes at most one `{ENUM_RESERVED_ATTRIBUTE}(..)` line; \
                         list every retired ordinal in the first one"
                    ),
                    span,
                ));
            }
            body.reserved = Some(reserved);
            continue;
        }
        body.variants.push(parse_variant(
            text,
            span,
            std::mem::take(&mut pending_docs),
        )?);
    }
    Ok(body)
}

fn parse_variant(
    text: &str,
    span: cratestack_core::SourceSpan,
    docs: Vec<String>,
) -> Result<EnumVariant, SchemaError> {
    let (name, ordinal) = match text.split_once('=') {
        Some((name, value)) => (name.trim_end(), Some(value.trim())),
        None => (text, None),
    };
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return Err(span_error(
            "enum variants must be declared as a single identifier per line, \
             optionally numbered as `Name = n`",
            span,
        ));
    }
    let name_span = cratestack_core::SourceSpan {
        end: span.start + name.len(),
        ..span
    };
    let ordinal = match ordinal {
        None => None,
        Some(value) => Some(value.parse::<i64>().map_err(|_| {
            span_error(
                format!(
                    "the ordinal of variant `{name}` must be an integer from 0 to \
                     {ENUM_ORDINAL_MAX}, written `{name} = 1`; `{value}` is not one"
                ),
                span,
            )
        })?),
    };
    Ok(EnumVariant {
        docs,
        name: name.to_owned(),
        ordinal,
        span: name_span,
    })
}

fn parse_reserved(
    text: &str,
    span: cratestack_core::SourceSpan,
) -> Result<ReservedOrdinals, SchemaError> {
    let Some(list) = text
        .strip_prefix(ENUM_RESERVED_ATTRIBUTE)
        .and_then(|rest| rest.trim_start().strip_prefix('('))
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        let name: String = text
            .chars()
            .take_while(|c| *c == '@' || c.is_alphanumeric() || *c == '_')
            .collect();
        return Err(span_error(
            format!(
                "an enum accepts no attribute but `{ENUM_RESERVED_ATTRIBUTE}(n, …)`, \
                 which retires ordinals; `{name}` is not supported on an enum"
            ),
            span,
        ));
    };
    let values = list
        .split(',')
        .map(str::trim)
        .map(|item| {
            item.parse::<i64>().map_err(|_| {
                span_error(
                    format!(
                        "`{ENUM_RESERVED_ATTRIBUTE}` lists integer ordinals, e.g. \
                         `{ENUM_RESERVED_ATTRIBUTE}(4, 7)`; `{item}` is not one"
                    ),
                    span,
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ReservedOrdinals { values, span })
}
