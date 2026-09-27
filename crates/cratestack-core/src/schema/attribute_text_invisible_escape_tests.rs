use super::{escape_for_diagnostic, substitute_for_display};

/// Every control character but line feed and tab, every character refused
/// in attribute text, and every variation selector is escaped: no such
/// character survives, and each appears as its `\u{…}` escape.
#[test]
fn no_control_or_invisible_character_survives_the_escape() {
    let escaped_chars = (0..=0x10FFFF_u32)
        .filter_map(char::from_u32)
        .filter(|&ch| {
            let text = format!("a{ch}b");
            let escaped = escape_for_diagnostic(&text);
            if escaped == text {
                return false;
            }
            assert_eq!(escaped, format!("a\\u{{{:X}}}b", ch as u32));
            true
        })
        .count();
    // 65 `Cc` less line feed and tab, the 4174 default-ignorable code
    // points (256 of them variation selectors), U+2800, U+1D159 and the 19
    // format controls DICP subtracts by name.
    assert_eq!(escaped_chars, 63 + 4174 + 1 + 1 + 19);
}

#[test]
fn nul_bel_and_esc_are_written_as_escapes() {
    assert_eq!(
        escape_for_diagnostic("@deny(hasRole(\"b\u{0}a\u{7}n\u{1B}[8m\"))"),
        "@deny(hasRole(\"b\\u{0}a\\u{7}n\\u{1B}[8m\"))"
    );
    assert_eq!(escape_for_diagnostic("x\ry\u{85}"), "x\\u{D}y\\u{85}");
    assert_eq!(
        escape_for_diagnostic("banné\u{FE0F} \u{2764}\u{FE0F}"),
        "banné\\u{FE0F} \u{2764}\\u{FE0F}"
    );
}

#[test]
fn visible_text_line_feeds_and_tabs_are_kept() {
    for text in ["café 中文 😀 שלום", "line\n\tnext", "\u{0600}١", "a\u{A0}b"] {
        assert_eq!(escape_for_diagnostic(text), text);
    }
}

/// The code-frame substitute keeps the text's length in characters, so a
/// span still points where it did, and leaves whitespace to the renderer.
#[test]
fn the_display_substitute_keeps_the_length_in_characters() {
    let source = "a\u{0}b\u{7}c\u{1B}d\u{7F}e\u{80}f\u{200B}g\u{1D159}h\u{FE0F}\ti\r\nj";
    let shown = substitute_for_display(source);
    assert_eq!(shown.chars().count(), source.chars().count());
    assert_eq!(
        shown,
        "a\u{2400}b\u{2407}c\u{241B}d\u{2421}e\u{FFFD}f\u{FFFD}g\u{FFFD}h\u{FFFD}\ti\r\nj"
    );
    assert!(
        !shown
            .chars()
            .any(|ch| ch.is_control() && !ch.is_whitespace())
    );
}
