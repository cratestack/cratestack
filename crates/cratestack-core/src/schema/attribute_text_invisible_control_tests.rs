//! Control characters in attribute text (module doc, fourth adjustment):
//! refused unless they are whitespace. And the names the diagnostics give
//! the refused characters.

use super::{describe_invisible_character, invisible_character};

/// Each C0 and C1 control that is not whitespace, inside a policy
/// literal, is refused at its own offset — NUL, BEL, ESC, DEL and U+0080
/// (the probe that checked `schema OK` before) among them.
#[test]
fn a_control_character_that_is_not_whitespace_is_refused() {
    let controls = ('\u{00}'..='\u{1F}').chain('\u{7F}'..='\u{9F}');
    let mut refused = 0;
    for ch in controls {
        let text = format!("@deny(hasRole(\"ban{ch}ned\"))");
        if matches!(ch, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}') {
            assert_eq!(invisible_character(&text), None, "U+{:04X}", ch as u32);
            continue;
        }
        assert_eq!(
            invisible_character(&text),
            Some((18, ch)),
            "U+{:04X}",
            ch as u32
        );
        refused += 1;
    }
    assert_eq!(refused, 59);
}

#[test]
fn the_description_names_a_control_character() {
    assert_eq!(
        describe_invisible_character('\u{80}'),
        "U+0080 (a control character)"
    );
    assert_eq!(
        describe_invisible_character('\u{1B}'),
        "U+001B (escape, which starts a terminal escape sequence)"
    );
}

#[test]
fn the_description_names_the_code_point() {
    assert_eq!(
        describe_invisible_character('\u{200B}'),
        "U+200B (zero width space)"
    );
    assert_eq!(
        describe_invisible_character('\u{034F}'),
        "U+034F (combining grapheme joiner)"
    );
    assert_eq!(
        describe_invisible_character('\u{E0100}'),
        "U+E0100 (a variation selector, refused in a policy attribute and elsewhere allowed \
         only right after a visible non-ASCII character such as an emoji)"
    );
    assert_eq!(
        describe_invisible_character('\u{1D159}'),
        "U+1D159 (musical symbol null notehead, which draws as blank)"
    );
    assert_eq!(
        describe_invisible_character('\u{FFF0}'),
        "U+FFF0 (a default-ignorable code point, which is displayed as nothing)"
    );
}
