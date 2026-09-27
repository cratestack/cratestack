//! Characters that make a line read differently from how it parses
//! (GHSA-69g4-xvcm-vm2j).
//!
//! The parser splits lines at `\n` only (`\r\n` included). A lone `\r`, a
//! vertical tab, a form feed, NEL, or the Unicode line and paragraph
//! separators are displayed by many editors and review tools as a line
//! break, but to the parser the text after one is still the same line. So
//! `@allow(x) // note` + lone CR + `@deny(y)` shows as two lines while the
//! `// note` comment swallows the `@deny`, and `// note` + lone CR +
//! `@deny(y)` is one comment line: either way the deny is silently gone.
//! Such a character is refused whenever anything but whitespace follows it
//! on the line; one that ends a line hides nothing and is left alone.
//!
//! Bidirectional override and isolate controls reorder how a line is
//! displayed, so a comment can be made to look like code and the other way
//! round (the "Trojan Source" attack); they are refused anywhere, as `rustc`
//! refuses them in comments and literals.
//!
//! For the same reason ESC (U+001B) and the C1 control sequence introducer
//! U+009B are refused anywhere, comments included: each starts a terminal
//! escape sequence, which can move the cursor or conceal text when the
//! schema is shown in a terminal, so a `// …` comment can be drawn over
//! with what looks like an applied `@deny`. In attribute text every control
//! character that is not whitespace is refused already
//! (`cratestack_core::schema::attribute_text::invisible_character`); this
//! covers the comment a trailing `//` holds and every other line.

use crate::diagnostics::SchemaError;
use crate::line_helpers::Line;

fn is_hidden_break(ch: char) -> bool {
    matches!(
        ch,
        '\r' | '\u{0B}' | '\u{0C}' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

fn is_bidi_control(ch: char) -> bool {
    matches!(ch, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn is_terminal_escape(ch: char) -> bool {
    matches!(ch, '\u{1B}' | '\u{9B}')
}

/// Refuses the first line carrying a hidden line break with text after it,
/// a bidirectional control or a terminal escape.
pub(super) fn refuse_hidden_breaks(lines: &[Line<'_>]) -> Result<(), SchemaError> {
    for line in lines {
        for (index, ch) in line.raw.char_indices() {
            let why = if is_bidi_control(ch) {
                "a bidirectional text control, which makes the line display in a different \
                 order from the one it is parsed in. Remove it"
            } else if is_terminal_escape(ch) {
                "a character that starts a terminal escape sequence, which lets a terminal \
                 display the line differently from how it is parsed, a comment as code or \
                 code hidden. Remove it"
            } else if is_hidden_break(ch) && !line.raw[index + ch.len_utf8()..].trim().is_empty() {
                "a character many editors display as a line break, which the schema parser \
                 does not treat as one: the text after it belongs to the same line, so after \
                 a `//` it is part of the comment and never applied. Replace it with an \
                 ordinary line break (`\\n` or `\\r\\n`)"
            } else {
                continue;
            };
            let start = line.start + index;
            return Err(SchemaError::new(
                format!(
                    "line {} contains U+{:04X}, {why}; the schema is refused until then",
                    line.number, ch as u32
                ),
                start..start + ch.len_utf8(),
                line.number,
            ));
        }
    }
    Ok(())
}
