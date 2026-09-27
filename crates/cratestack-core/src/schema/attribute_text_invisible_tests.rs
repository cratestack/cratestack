use super::table::DEFAULT_IGNORABLE;
use super::{describe_invisible_character, invisible_character, is_default_ignorable};

fn all_chars() -> impl Iterator<Item = char> {
    (0..=0x10FFFF_u32).filter_map(char::from_u32)
}

/// `DerivedCoreProperties-16.0.0.txt` has 27 `Default_Ignorable_Code_Point`
/// lines totalling 4174 code points, so a range dropped from, widened in or
/// added to the table shows here.
#[test]
fn the_table_is_unicode_16_default_ignorable_code_point() {
    assert_eq!(DEFAULT_IGNORABLE.len(), 27);
    for pair in DEFAULT_IGNORABLE.windows(2) {
        assert!(pair[0].1 < pair[1].0, "{pair:?} not sorted and disjoint");
    }
    let total: u32 = DEFAULT_IGNORABLE
        .iter()
        .map(|&(first, last)| last as u32 - first as u32 + 1)
        .sum();
    assert_eq!(total, 4174);
    assert_eq!(
        all_chars().filter(|&ch| is_default_ignorable(ch)).count(),
        4174
    );
}

/// Every code point, alone after an ASCII letter: the 4174 default-ignorable
/// ones, U+2800, U+1D159, the 19 format controls DICP subtracts by name
/// (U+FFF9–U+FFFB, U+13430–U+1343F) and the 59 control characters that
/// are not whitespace (65 `Cc` less tab, LF, VT, FF, CR and NEL) are
/// refused, nothing else — 4254. After a visible non-ASCII character, in
/// text that is not a policy attribute, the 256 variation selectors are
/// allowed as well.
#[test]
fn exactly_the_table_the_blank_symbols_the_named_format_controls_and_controls_are_refused() {
    let refused_after = |base: char| {
        all_chars()
            .filter(|&ch| {
                let text = format!("{base}{ch}");
                invisible_character(&text) == Some((base.len_utf8(), ch))
            })
            .count()
    };
    assert_eq!(refused_after('n'), 4174 + 1 + 1 + 3 + 16 + 59);
    assert_eq!(refused_after('中'), 4174 + 1 + 1 + 3 + 16 + 59 - 256);
}

/// The `Cf` format controls `Default_Ignorable_Code_Point` subtracts by
/// name (`- FFF9..FFFB - 13430..13440`): no glyph of their own, so each is
/// refused, every one of them, while U+13440 — a combining mark in that
/// range, not a format control — is not.
#[test]
fn the_format_controls_dicp_subtracts_are_refused() {
    let controls = ('\u{FFF9}'..='\u{FFFB}').chain('\u{13430}'..='\u{1343F}');
    let mut count = 0;
    for ch in controls {
        assert!(!is_default_ignorable(ch), "U+{:04X}", ch as u32);
        let text = format!("@deny(hasRole(\"ban{ch}ned\"))");
        assert_eq!(
            invisible_character(&text),
            Some((18, ch)),
            "U+{:04X}",
            ch as u32
        );
        count += 1;
    }
    assert_eq!(count, 19);
    assert_eq!(invisible_character("\u{13000}\u{13440}"), None);
    assert_eq!(
        describe_invisible_character('\u{FFF9}'),
        "U+FFF9 (an interlinear annotation control)"
    );
    assert_eq!(
        describe_invisible_character('\u{13430}'),
        "U+13430 (an Egyptian hieroglyph format control)"
    );
}

/// The characters measured letting a `@deny` through, and the zero-width
/// ones the earlier `Cf` rule already refused.
#[test]
fn invisible_characters_are_refused() {
    for ch in [
        '\u{034F}',
        '\u{3164}',
        '\u{115F}',
        '\u{1160}',
        '\u{FFA0}',
        '\u{180B}',
        '\u{180C}',
        '\u{180D}',
        '\u{180E}',
        '\u{180F}',
        '\u{17B4}',
        '\u{17B5}',
        '\u{FE0F}',
        '\u{FE00}',
        '\u{E0100}',
        '\u{E01EF}',
        '\u{2800}',
        '\u{1D159}',
        '\u{200B}',
        '\u{200C}',
        '\u{200D}',
        '\u{2060}',
        '\u{FEFF}',
        '\u{AD}',
        '\u{E0041}',
        '\u{202E}',
        '\u{2065}',
    ] {
        let text = format!("@deny(hasRole(\"ban{ch}ned\"))");
        assert_eq!(
            invisible_character(&text),
            Some((18, ch)),
            "U+{:04X}",
            ch as u32
        );
    }
}

#[test]
fn outside_a_policy_a_variation_selector_needs_a_visible_non_ascii_character_before_it() {
    for text in [
        "\u{2764}\u{FE0F}",
        "\u{9F8D}\u{FE00}",
        "\u{8FBA}\u{E0100}",
        "©\u{FE0F}",
    ] {
        assert_eq!(invisible_character(text), None, "{text:?}");
    }
    for (text, at) in [
        ("n\u{FE0F}", 1),
        ("\u{FE0F}", 0),
        ("1\u{FE0F}\u{20E3}", 1),
        ("\u{2764}\u{FE0F}\u{FE0F}", 6),
        ("\u{A0}\u{FE0F}", 2),
        ("\u{3000}\u{FE0F}", 3),
        ("\u{2800}\u{FE0F}", 0),
        ("\u{1D159}\u{FE0F}", 0),
    ] {
        assert_eq!(
            invisible_character(text).map(|(i, _)| i),
            Some(at),
            "{text:?}"
        );
    }
}

#[test]
fn visible_text_is_allowed() {
    for text in [
        "café 中文 😀",
        "한국어 텍스트",
        "שלום עולם",
        "مرحبا بالعالم",
        "日本語のテキスト",
        "Ærø ß",
        "a\u{A0}b",
        // Visible `Cf` characters, not default-ignorable: the Arabic number
        // signs and the other prepended concatenation marks — all 13 of
        // Unicode 16's, the only `Cf` characters allowed.
        "\u{0600}\u{0601}\u{0602}\u{0603}\u{0604}\u{0605}١٢",
        "\u{06DD}\u{070F}\u{0890}\u{0891}\u{08E2}\u{110BD}\u{110CD}",
    ] {
        assert_eq!(invisible_character(text), None, "{text:?}");
    }
}

/// Zero-width joiners stay refused, so a joined emoji is refused too.
#[test]
fn a_joined_emoji_is_refused() {
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    assert_eq!(invisible_character(family), Some((4, '\u{200D}')));
}
