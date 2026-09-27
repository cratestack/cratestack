//! The name a diagnostic gives an invisible character (the parent
//! module's doc says which characters are refused, and why).

use super::{BRAILLE_PATTERN_BLANK, MUSICAL_SYMBOL_NULL_NOTEHEAD};

/// `U+200B (zero width space)`: the code point, with a name for the ones
/// most likely to be met.
pub fn describe_invisible_character(ch: char) -> String {
    let name = match ch {
        '\u{00AD}' => "soft hyphen",
        '\u{034F}' => "combining grapheme joiner",
        '\u{115F}' => "hangul choseong filler",
        '\u{1160}' => "hangul jungseong filler",
        '\u{3164}' => "hangul filler",
        '\u{FFA0}' => "halfwidth hangul filler",
        '\u{17B4}' | '\u{17B5}' => "a Khmer inherent vowel, which is not displayed",
        '\u{180B}'..='\u{180D}' | '\u{180F}' => "a Mongolian free variation selector",
        '\u{180E}' => "mongolian vowel separator",
        '\u{200B}' => "zero width space",
        '\u{200C}' => "zero width non-joiner",
        '\u{200D}' => "zero width joiner",
        '\u{200E}' => "left-to-right mark",
        '\u{200F}' => "right-to-left mark",
        '\u{2060}' => "word joiner",
        '\u{FEFF}' => "zero width no-break space / byte-order mark",
        '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{061C}' => {
            "a bidirectional text control"
        }
        '\u{E0020}'..='\u{E007F}' => "a tag character",
        '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}' => {
            "a variation selector, refused in a policy attribute and elsewhere allowed only \
             right after a visible non-ASCII character such as an emoji"
        }
        BRAILLE_PATTERN_BLANK => "braille pattern blank",
        MUSICAL_SYMBOL_NULL_NOTEHEAD => "musical symbol null notehead, which draws as blank",
        '\u{FFF9}'..='\u{FFFB}' => "an interlinear annotation control",
        '\u{13430}'..='\u{1343F}' => "an Egyptian hieroglyph format control",
        '\u{1B}' => "escape, which starts a terminal escape sequence",
        '\u{00}'..='\u{1F}' | '\u{7F}'..='\u{9F}' => "a control character",
        _ => "a default-ignorable code point, which is displayed as nothing",
    };
    format!("U+{:04X} ({name})", ch as u32)
}
