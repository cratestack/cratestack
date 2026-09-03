//! `query <name>(<arg>: <Type>, ...): <ResultType>` block parser
//! (cratestack#867; design `docs/design/declarative-custom-query.md` §8).
//!
//! The header is deliberately spelled exactly like a `procedure`'s — same
//! parenthesised arg list, same `: ReturnType` suffix — and reuses
//! [`parse_procedure_args`] verbatim, because a `query`'s parameters *are*
//! a procedure's parameters as far as everything downstream is concerned
//! (the policy resolver included; see `cratestack_core::schema::query`).
//! The body is not a `{ … }` block: everything after the header is
//! attribute lines, of which `@@sql(…)` carries the SQL and
//! `@allow`/`@deny` carry the policy.
//!
//! Why no `{ … }` braces: a `query` has no field list to enclose. Its
//! result shape is a *reference* to a `type` declaration (design §3), not
//! an inline body, so a brace block would enclose nothing but attributes
//! that every other attribute-only construct in the language
//! (`procedure`) already writes unbraced.

use cratestack_core::{Attribute, Query, SourceSpan};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, name_span_in_line};
use crate::parse::procedures::parse_procedure_args;
use crate::parse::sql_attribute::collect_attribute_text;
use crate::parse::types::parse_type_ref;

pub(super) fn parse_query(
    lines: &[Line<'_>],
    start: usize,
    docs: Vec<String>,
) -> Result<(Query, usize), SchemaError> {
    let line = &lines[start];
    let signature = line.trimmed.strip_prefix("query ").ok_or_else(|| {
        SchemaError::new(
            "expected query declaration",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    let open_paren = signature.find('(').ok_or_else(|| {
        SchemaError::new(
            "query declaration must include parameter parentheses, e.g. \
             `query totals(userId: String): Totals`",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;
    let close_paren = signature.rfind(')').ok_or_else(|| {
        SchemaError::new(
            "query declaration must close parameter parentheses",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    let name = signature[..open_paren].trim();
    if name.is_empty() {
        return Err(SchemaError::new(
            "query declaration missing name",
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    }
    let args_src = signature[open_paren + 1..close_paren].trim();
    let result_src = signature[close_paren + 1..]
        .trim()
        .strip_prefix(':')
        .map(str::trim)
        .ok_or_else(|| {
            SchemaError::new(
                "query declaration must include a result type, e.g. \
                 `query totals(userId: String): Totals` (or `: Totals[]` for many rows)",
                line.start..line.start + line.raw.len(),
                line.number,
            )
        })?;
    if result_src.is_empty() {
        return Err(SchemaError::new(
            "query declaration must include a result type after `:`",
            line.start..line.start + line.raw.len(),
            line.number,
        ));
    }

    let (attributes, cursor) = collect_query_attributes(lines, start + 1)?;

    let name_span = name_span_in_line(line, line.trimmed, "query ")?;
    let result_type_offset = line.raw.rfind(result_src).ok_or_else(|| {
        SchemaError::new(
            "failed to locate result type in query declaration",
            line.start..line.start + line.raw.len(),
            line.number,
        )
    })?;

    Ok((
        Query {
            docs,
            name: name.to_owned(),
            name_span,
            // `parse_procedure_args` wants per-argument doc comments keyed
            // by name; a `query`'s args have none to key (there is no
            // `@param`-style convention on this construct in v1), so an
            // empty map is passed rather than inventing one.
            args: parse_procedure_args(args_src, line, &Default::default())?,
            result_type: parse_type_ref(result_src, line, result_type_offset)?,
            attributes,
            span: SourceSpan {
                start: line.start,
                end: line.start + line.raw.len(),
                line: line.number,
            },
        },
        cursor,
    ))
}

/// Attribute lines following the header, up to the first line that is
/// neither blank nor `@…`-prefixed.
///
/// Same shape as `parse_procedure`'s inline attribute loop, with one
/// addition: `@@sql("""…""")` may span physical lines, so each attribute
/// is taken through [`collect_attribute_text`] instead of being read as a
/// single trimmed line.
fn collect_query_attributes(
    lines: &[Line<'_>],
    start: usize,
) -> Result<(Vec<Attribute>, usize), SchemaError> {
    let mut attributes = Vec::new();
    let mut cursor = start;
    while cursor < lines.len() {
        let candidate = &lines[cursor];
        if candidate.trimmed.starts_with('@') {
            let (raw, span, next) = collect_attribute_text(lines, cursor, "query")?;
            attributes.push(Attribute { raw, span });
            cursor = next;
            continue;
        }
        if candidate.trimmed.is_empty() {
            cursor += 1;
            continue;
        }
        break;
    }
    Ok((attributes, cursor))
}
