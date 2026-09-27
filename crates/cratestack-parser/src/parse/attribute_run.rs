//! The attribute lines under a `procedure` or `query` signature
//! (GHSA-69g4-xvcm-vm2j).
//!
//! These attributes carry authorization (`@allow`, `@deny`,
//! `@authorize`), and are written below the signature with no braces, so
//! which declaration they belong to is decided by layout alone. Three
//! rules make that decision unambiguous:
//!
//! 1. **A run is the lines under the signature, up to the first blank
//!    line.** Comment and doc-comment lines (`//`, `///`) inside it are
//!    skipped and do not end it, including one between the signature and
//!    its first attribute (maintainer decision); any other line that is
//!    not an attribute ends it too. Before, blank lines were skipped as
//!    well, so a `@deny(...)` written above the *next* procedure, after a
//!    blank line, silently became part of the previous procedure and the
//!    one it was written for lost it.
//! 2. **A group that is not directly under a signature is refused**
//!    ([`detached_group_error`]), naming the declarations on either side.
//! 3. **A run may not lead straight into another declaration.** With no
//!    blank line between them, its last lines sit directly above that
//!    declaration and read as its attributes. Comment and doc-comment
//!    lines in between do not separate them: `@deny(x)` followed by
//!    `/// docs for b` and then `procedure b` is refused too, since the
//!    deny reads as b's. Every committed schema, example and doc
//!    separates declarations with a blank line.
//!
//! Each line is also stripped of its trailing `//` comment and cut into
//! one attribute per `@name` (`@no_idempotency @deny(x)` is two), so an
//! attribute sharing a line is read rather than folded into its
//! neighbour's text. Cutting only happens where whitespace precedes the
//! `@`; `@deny(x)@allow(y)` stays whole and is refused by the validator.

use cratestack_core::schema::attribute_text::split_attributes;
use cratestack_core::{Attribute, SourceSpan};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, attribute_line, joined_offset_in_source};
use crate::parse::format_chars::refuse_invisible_characters;
use crate::parse::sql_attribute::collect_attribute_text;

/// Collects the run starting at `lines[start]` for `owner` (e.g.
/// ``procedure `transfer` ``). `construct` is `Some("query")` when an
/// attribute may be a multi-line `@@sql("""…""")` body.
pub(super) fn collect_attribute_run(
    lines: &[Line<'_>],
    start: usize,
    owner: &str,
    construct: Option<&str>,
) -> Result<(Vec<Attribute>, usize), SchemaError> {
    let mut attributes = Vec::new();
    let mut cursor = start;
    // Rule 1: comment lines inside the run are skipped, and the run goes
    // on while an attribute line follows them.
    while let Some(at) = next_attribute_line(lines, cursor) {
        cursor = at;
        let (raw, span, next) = match construct {
            Some(construct) => collect_attribute_text(lines, cursor, construct)?,
            None => {
                let (raw, span) = attribute_line(&lines[cursor]);
                (raw.to_owned(), span, cursor + 1)
            }
        };
        attributes.extend(split_line(&raw, span, &lines[cursor..next]));
        cursor = next;
    }
    refuse_invisible_characters(&attributes, lines)?;
    // Comment and doc-comment lines do not separate: `@deny(x)`, then
    // `/// docs for b`, then `procedure b` reads as b's deny just as much.
    let after_comments = (cursor..lines.len()).find(|&at| !lines[at].trimmed.starts_with("//"));
    if let (Some(last), Some(line)) = (attributes.last(), after_comments.map(|at| &lines[at]))
        && let Some(next) = declaration_label(line.trimmed)
    {
        let starts = if after_comments == Some(cursor) {
            "starts on the very next line"
        } else {
            "follows it with only comment lines in between"
        };
        return Err(SchemaError::new(
            format!(
                "`{}` is the last attribute of {owner}, and {next} {starts}, so it reads as \
                 belonging to {next} as much as to {owner}. Put a blank line between them: \
                 the attributes of a procedure or query are the lines under its own \
                 signature up to the first blank line, comment lines included",
                last.raw
            ),
            last.span.start..last.span.end,
            last.span.line,
        ));
    }
    Ok((attributes, cursor))
}

/// Where the run continues from `cursor`: the first line from there that
/// is not a comment line, when it is an attribute line. `None` when it
/// is blank, is anything else, or the text ends first.
fn next_attribute_line(lines: &[Line<'_>], cursor: usize) -> Option<usize> {
    let at = (cursor..lines.len()).find(|&at| !lines[at].trimmed.starts_with("//"))?;
    lines[at].trimmed.starts_with('@').then_some(at)
}

/// One [`Attribute`] per `@name` on the collected text, which starts at
/// `span.start` on the first of `lines` (all of them, for a multi-line
/// `"""` body).
fn split_line(raw: &str, span: SourceSpan, lines: &[Line<'_>]) -> Vec<Attribute> {
    let lead = span.start - lines[0].start;
    split_attributes(raw)
        .into_iter()
        .map(|(offset, text)| {
            let (start, line) = joined_offset_in_source(lines, lead + offset);
            let (end, _) = joined_offset_in_source(lines, lead + offset + text.len());
            Attribute {
                raw: text.to_owned(),
                span: SourceSpan { start, end, line },
            }
        })
        .collect()
}

/// ``procedure `name` ``, ``model `name` ``, … when `trimmed` is the
/// header of a top-level declaration.
pub(super) fn declaration_label(trimmed: &str) -> Option<String> {
    const KEYWORDS: &[(&str, &str)] = &[
        ("mutation procedure ", "procedure"),
        ("procedure ", "procedure"),
        ("query ", "query"),
        ("model ", "model"),
        ("view ", "view"),
        ("type ", "type"),
        ("enum ", "enum"),
        ("mixin ", "mixin"),
        ("auth ", "auth"),
        ("datasource ", "datasource"),
        ("extension ", "extension"),
    ];
    if trimmed == "mcp {" {
        return Some("the `mcp` block".to_owned());
    }
    if trimmed == "transport" || trimmed.starts_with("transport ") {
        return Some("the `transport` directive".to_owned());
    }
    KEYWORDS.iter().find_map(|(prefix, kind)| {
        let name = trimmed
            .strip_prefix(prefix)?
            .split(|c: char| c == '(' || c == '{' || c.is_whitespace())
            .next()
            .filter(|name| !name.is_empty())?;
        Some(format!("{kind} `{name}`"))
    })
}

/// The error for an attribute line met at the top level: one that no
/// signature directly above claims. `previous` is the declaration before
/// it, if any.
pub(super) fn detached_group_error(
    lines: &[Line<'_>],
    at: usize,
    previous: Option<&str>,
) -> SchemaError {
    let line = &lines[at];
    let next = lines[at..]
        .iter()
        .map(|line| line.trimmed)
        .find(|trimmed| {
            !trimmed.is_empty() && !trimmed.starts_with('@') && !trimmed.starts_with("//")
        })
        .and_then(declaration_label);
    let place = match (previous, next.as_deref()) {
        (Some(previous), Some(next)) => format!("between {previous} and {next}"),
        (Some(previous), None) => format!("after {previous}"),
        (None, Some(next)) => format!("above {next}"),
        (None, None) => "at the top level".to_owned(),
    };
    let (text, span) = attribute_line(line);
    // A model's or view's `@@…` (not a query's `@@sql`) outside its braces:
    // no procedure or query ever takes one, so point at the body instead.
    if text.starts_with("@@") && !text.starts_with("@@sql") {
        return SchemaError::new(
            format!(
                "`{text}` stands {place}, outside any `model` or `view` body: a `@@` block \
                 attribute belongs inside the `{{ … }}` of the model or view it applies to"
            ),
            span.start..span.end,
            span.line,
        );
    }
    SchemaError::new(
        format!(
            "`{text}` is not directly under a signature: it stands {place}, separated from \
             any signature by a blank line or another declaration, so which declaration it \
             belongs to is ambiguous and it is refused (before, a group separated only by \
             blank lines was silently attached to the procedure or query above it). Write a \
             procedure's or query's attributes on the lines under its own signature, with no \
             blank line in between (comment lines are fine)"
        ),
        span.start..span.end,
        span.line,
    )
}
