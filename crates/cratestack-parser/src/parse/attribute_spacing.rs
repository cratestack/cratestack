//! Splits a field's trailing attribute text into individual `@attribute`
//! tokens, rejecting an argument group that is separated from its attribute
//! name by whitespace.
//!
//! `@computed (params: ProxyParams?)` used to silently parse as bare
//! `@computed`: the caller's loop pushed the completed attribute at the
//! whitespace, then — with the accumulator empty again — nothing but `@`
//! restarts an attribute, so `(params: ProxyParams?)` matched no branch and
//! vanished with no diagnostic. This module rejects that shape instead of
//! attaching the group to the preceding attribute: attaching would silently
//! *change* the meaning of any schema that parses today (as a bare
//! attribute with the group ignored), whereas rejecting cannot change any
//! working schema's meaning — a dropped argument group was always a bug,
//! and the fix is one keystroke. Fail closed.
//!
//! Out of scope, deliberately: a group with **no** preceding attribute at
//! all (e.g. `name String (foo)`) keeps today's behaviour of being silently
//! dropped — see `pending_gap` below, which is only armed by a just-closed
//! `@attribute`, never by plain leading text.
//!
//! The tokenizer is quote-aware: inside a double-quoted string literal
//! (e.g. `@default("a) (b")`), `(`/`)`/`[`/`]`/whitespace/`@` are ordinary
//! characters — they affect neither paren/bracket depth nor attribute
//! splitting nor `pending_gap`. Without this, a string value containing an
//! unbalanced paren desynchronizes `depth` from the real group nesting,
//! which manifests two ways: a legitimately-attached `@default` with such a
//! value gets misdiagnosed as whitespace-separated, and — worse — a
//! following attribute's whitespace-delimited split is silently skipped
//! (because `depth` never returns to `0`), swallowing the next attribute
//! into the previous one's raw text with no diagnostic at all. Neither the
//! rest of this parser nor `cratestack-core`'s composite-attribute parsing
//! has any notion of escape sequences inside string literals, so this
//! module doesn't invent one either — a literal `"` always toggles the
//! in-string flag, with no backslash handling.

use crate::diagnostics::SchemaError;
use crate::line_helpers::Line;

pub(super) fn split_field_attributes(
    attrs: &str,
    offset: usize,
    field_name: &str,
    line: &Line<'_>,
) -> Result<Vec<(String, usize, usize)>, SchemaError> {
    let mut attributes = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut current_start = None;
    // The most recently completed `@attribute`'s raw text, kept alive only
    // while nothing but whitespace has followed it. A `(`/`[` arriving while
    // this is armed is the whitespace-separated-group bug; anything else
    // (a new `@attribute`, or stray non-whitespace text) disarms it.
    let mut pending_gap: Option<String> = None;
    // Whether we're inside a double-quoted string literal in the attribute
    // currently being accumulated. While armed, every character other than
    // the closing `"` is ordinary text — see the module doc for why.
    let mut in_string = false;

    for (index, ch) in attrs.char_indices() {
        if current.is_empty() {
            if ch == '@' {
                current.push(ch);
                current_start = Some(offset + index);
                pending_gap = None;
                continue;
            }
            if (ch == '(' || ch == '[') && pending_gap.is_some() {
                let attr_raw = pending_gap.take().unwrap_or_default();
                let group_len = attribute_group_end(&attrs[index..]);
                let group = &attrs[index..index + group_len];
                let start = line.start + offset + index;
                return Err(SchemaError::new(
                    format!(
                        "field `{field_name}`: attribute arguments must not be separated from \
                         the attribute name by whitespace — write `{attr_raw}{group}`, not \
                         `{attr_raw} {group}`",
                    ),
                    start..start + group_len,
                    line.number,
                ));
            }
            if !ch.is_whitespace() {
                pending_gap = None;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = !in_string;
                current.push(ch);
            }
            '(' | '[' if !in_string => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' if !in_string => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ch if ch.is_whitespace() && depth == 0 && !in_string => {
                let start = current_start.take().unwrap_or(offset + index);
                pending_gap = Some(current.clone());
                attributes.push((std::mem::take(&mut current), start, offset + index));
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        let start = current_start.unwrap_or(offset + attrs.len().saturating_sub(current.len()));
        attributes.push((current, start, offset + attrs.len()));
    }

    Ok(attributes)
}

/// Given a string slice starting at a `(` or `[`, returns the byte length of
/// the balanced group (through its matching `)`/`]`). Falls back to the
/// whole slice if the group never closes — callers only reach here from an
/// already-tokenized attribute list, so an unclosed group is not expected,
/// but this keeps the span computation total rather than panicking.
///
/// Quote-aware for the same reason as `split_field_attributes`: a `(`/`)`/
/// `[`/`]` inside a double-quoted string literal in the offending group
/// must not perturb the depth count.
fn attribute_group_end(text: &str) -> usize {
    let mut depth = 0usize;
    let mut in_string = false;
    for (index, ch) in text.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '(' | '[' if !in_string => depth += 1,
            ')' | ']' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return index + ch.len_utf8();
                }
            }
            _ => {}
        }
    }
    text.len()
}
