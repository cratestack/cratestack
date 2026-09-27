//! Lexical scanning of `.cstack` attribute text: where string literals
//! are, where a trailing `//` comment starts, and where one attribute
//! ends and the next begins.
//!
//! Shared by `cratestack-parser` (which strips comments and splits
//! attributes when it builds [`super::Attribute::raw`], and refuses
//! shapes no generator reads) and `cratestack-macros` (which re-checks
//! every policy attribute before generating code, GHSA-69g4-xvcm-vm2j),
//! so the two cannot disagree about what is inside a string.
//!
//! Three string forms, matching what the readers accept:
//!
//! - `"""…"""` is verbatim (a SQL body, `super::sql_body`): nothing
//!   escapes, a lone `"` inside is ordinary, only `"""` closes it.
//! - `"…"` and `'…'` (policy literals may be single-quoted): only the
//!   same quote closes, and a backslash escapes the next character.
//!
//! Outside strings, `(`/`)` and `[`/`]` nest.
//!
//! Invisible characters are refused in attribute text wherever they
//! stand, strings included ([`invisible_character`]), and a diagnostic
//! never quotes one raw ([`escape_for_diagnostic`]).

#[path = "attribute_text_invisible.rs"]
mod invisible;

pub use invisible::{
    describe_invisible_character, escape_for_diagnostic, invisible_character,
    invisible_character_in_policy, is_default_ignorable, is_policy_attribute,
    substitute_for_display,
};

/// A character of attribute text that lies outside every string literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unquoted {
    /// Byte offset of the character in the scanned text.
    pub index: usize,
    pub ch: char,
    /// `(`/`[` nesting depth before this character.
    pub depth: usize,
}

enum Quote {
    Triple,
    Single(char),
}

/// Every character of `raw` outside a string literal, in order. Quote
/// characters themselves are not included.
pub fn unquoted(raw: &str) -> Vec<Unquoted> {
    let mut found = Vec::new();
    let mut quote: Option<Quote> = None;
    let mut escaped = false;
    let mut depth = 0usize;
    let mut skip = 0usize;
    for (index, ch) in raw.char_indices() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        match quote {
            Some(Quote::Triple) => {
                if raw[index..].starts_with("\"\"\"") {
                    quote = None;
                    skip = 2;
                }
            }
            Some(Quote::Single(open)) => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == open {
                    quote = None;
                }
            }
            None if raw[index..].starts_with("\"\"\"") => {
                quote = Some(Quote::Triple);
                skip = 2;
            }
            None if ch == '"' || ch == '\'' => quote = Some(Quote::Single(ch)),
            None => {
                found.push(Unquoted { index, ch, depth });
                match ch {
                    '(' | '[' => depth += 1,
                    ')' | ']' => depth = depth.saturating_sub(1),
                    _ => {}
                }
            }
        }
    }
    found
}

/// Byte offset of the first `//` outside a string literal: the start of
/// a trailing comment, which runs to the end of the line.
pub fn comment_start(raw: &str) -> Option<usize> {
    unquoted(raw)
        .windows(2)
        .find(|pair| pair[0].ch == '/' && pair[1].ch == '/' && pair[1].index == pair[0].index + 1)
        .map(|pair| pair[0].index)
}

/// `raw` without its trailing `//` comment, if any, and without the
/// whitespace before it.
pub fn strip_comment(raw: &str) -> &str {
    match comment_start(raw) {
        Some(start) => raw[..start].trim_end(),
        None => raw,
    }
}

/// Byte offsets of every `@` in `raw` that starts another attribute: one
/// outside any string literal and any `(...)`/`[...]` group, after the
/// text's own leading `@`s, and not the second `@` of an `@@`.
pub fn attribute_starts(raw: &str) -> Vec<usize> {
    let sigils = raw.len() - raw.trim_start_matches('@').len();
    unquoted(raw)
        .into_iter()
        .filter(|c| c.index >= sigils && c.ch == '@' && c.depth == 0)
        .filter(|c| !raw[..c.index].ends_with('@'))
        .map(|c| c.index)
        .collect()
}

/// `raw` cut into attributes at every [`attribute_starts`] offset that
/// whitespace precedes, each with its byte offset in `raw` and trimmed.
/// An `@` with no whitespace before it (`@deny(x)@allow(y)`) does not
/// cut, so the run-together text reaches the validators whole and is
/// refused there with a message naming both halves.
pub fn split_attributes(raw: &str) -> Vec<(usize, &str)> {
    let mut cuts = attribute_starts(raw)
        .into_iter()
        .filter(|&index| raw[..index].ends_with(char::is_whitespace))
        .collect::<Vec<_>>();
    cuts.push(raw.len());
    let mut pieces = Vec::with_capacity(cuts.len());
    let mut start = 0;
    for cut in cuts {
        let piece = &raw[start..cut];
        let lead = piece.len() - piece.trim_start().len();
        if !piece.trim().is_empty() {
            pieces.push((start + lead, piece.trim()));
        }
        start = cut;
    }
    pieces
}

/// Byte offset just past the `)`/`]` that closes the group opened at
/// `open` (which must be a `(` or `[` outside any string), or `None`
/// when the text ends first.
pub fn group_end(raw: &str, open: usize) -> Option<usize> {
    let mut chars = unquoted(raw).into_iter().skip_while(|c| c.index < open);
    let opening = chars.next().filter(|c| c.index == open)?;
    chars
        .find(|c| c.depth == opening.depth + 1 && matches!(c.ch, ')' | ']'))
        .map(|c| c.index + 1)
}

/// The name of every attribute in `raw`, read loosely: after each run
/// of `@`s outside strings and groups, whitespace is skipped, the text up
/// to the next `(`, whitespace, `@` or quote is taken, everything in it
/// but letters, digits and `_` is dropped, and the rest is lower-cased.
/// A default-ignorable character is dropped even when it counts as a
/// letter (the Hangul filler U+3164 does).
/// `@ Deny (x)` gives `deny`, and so do `@@de\u{200B}ny(x)` (a zero-width
/// space) and `@@de-ny(x)`: an invisible or punctuation character inside
/// the name must not turn a policy into an unknown, inert attribute. For
/// re-checking that no attribute a reader would take for a policy was
/// skipped, whatever its spelling.
pub fn loose_attribute_names(raw: &str) -> Vec<String> {
    let mut starts = attribute_starts(raw);
    if raw.starts_with('@') {
        starts.insert(0, 0);
    }
    starts
        .into_iter()
        .map(|start| {
            raw[start..]
                .trim_start_matches('@')
                .trim_start()
                .chars()
                .take_while(|c| !(c.is_whitespace() || matches!(c, '(' | '@' | '"' | '\'')))
                .filter(|c| (c.is_alphanumeric() || *c == '_') && !is_default_ignorable(*c))
                .flat_map(char::to_lowercase)
                .collect()
        })
        .collect()
}

#[cfg(test)]
#[path = "attribute_text_tests.rs"]
mod tests;

#[path = "attribute_text_names.rs"]
mod names;
pub use names::{
    PRIMARY_KEY_ATTRIBUTE, field_attribute_name, is_primary_key_attribute, is_relation_attribute,
};
