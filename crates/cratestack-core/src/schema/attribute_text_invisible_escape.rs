//! Diagnostics never write an invisible character raw (GHSA-69g4-xvcm-vm2j,
//! decided under the maintainer's standing secure-default rule).
//!
//! An error that quotes the attribute text or the line it refuses would
//! otherwise write the very character it refuses to the terminal, the
//! editor or the compiler output: a NUL, a BEL (the terminal beeps), an
//! ESC that starts an escape sequence (it can recolour, conceal or rewrite
//! what follows, the diagnostic included), a bidirectional control that
//! reorders the quoted text, or a zero-width character that makes the
//! quote look exactly like the accepted spelling. So every diagnostic
//! shows such a character as a visible stand-in instead:
//!
//! - [`escape_for_diagnostic`] is for the message text: each character is
//!   written as `\u{…}` (BEL is `\u{7}`, the zero-width space `\u{200B}`),
//!   so the reader sees what and where it is. Line feed and tab stay as
//!   they are — layout, and nothing a terminal acts on — and every other
//!   control character is escaped, whitespace or not (a lone carriage
//!   return moves the cursor back over the message). Every variation
//!   selector is escaped too, even after an emoji: the one refused in a
//!   policy (`"banné\u{FE0F}"`) would otherwise be quoted invisibly.
//! - [`substitute_for_display`] is for a quoted copy of the source, such
//!   as the code frame under a rendered error, where the text must keep
//!   its length in characters so that the spans still point at the right
//!   place: each character becomes one visible character — the Unicode
//!   control picture for a C0 control or DEL (`␀`, `␇`, `␛`, `␡`), U+FFFD
//!   for anything else. Whitespace is left to the renderer, which draws it
//!   as a space or breaks the line there.
//!
//! A backslash already in the text is not escaped, so source text that
//! spells `\u{7}` literally quotes the same as an escaped BEL; the error's
//! own `U+…` description and column tell the two apart.

use super::{is_always_invisible, is_variation_selector};

/// Whether a diagnostic's message must not contain `ch` raw.
fn escaped_in_message(ch: char) -> bool {
    (ch.is_control() && !matches!(ch, '\n' | '\t'))
        || is_always_invisible(ch)
        || is_variation_selector(ch)
}

/// `text` with every invisible or control character except line feed and
/// tab written as `\u{…}` (module doc).
pub fn escape_for_diagnostic(text: &str) -> String {
    if !text.chars().any(escaped_in_message) {
        return text.to_owned();
    }
    let mut escaped = String::with_capacity(text.len() + 8);
    for ch in text.chars() {
        if escaped_in_message(ch) {
            escaped.push_str(&format!("\\u{{{:X}}}", ch as u32));
        } else {
            escaped.push(ch);
        }
    }
    escaped
}

/// `text` with every invisible character that is not whitespace replaced
/// by one visible character, so it keeps its length in characters (module
/// doc).
pub fn substitute_for_display(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{00}'..='\u{1F}' if !ch.is_whitespace() => {
                char::from_u32(0x2400 + ch as u32).unwrap_or('\u{FFFD}')
            }
            '\u{7F}' => '\u{2421}',
            _ if is_always_invisible(ch) || is_variation_selector(ch) => '\u{FFFD}',
            _ => ch,
        })
        .collect()
}

#[cfg(test)]
#[path = "attribute_text_invisible_escape_tests.rs"]
mod tests;
