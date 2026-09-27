//! Unicode 16.0 `Default_Ignorable_Code_Point`, one entry per data line of
//! `DerivedCoreProperties-16.0.0.txt` (Unicode Character Database, dated
//! 2024-05-31; <https://www.unicode.org/Public/16.0.0/ucd/DerivedCoreProperties.txt>,
//! SHA-256 `39d35161f2954497f69e08bdb9e701493f476a3d30222de20028feda36c1dabd`),
//! whose derivation is `Other_Default_Ignorable_Code_Point + Cf +
//! Variation_Selector - White_Space - FFF9..FFFB - 13430..13440 -
//! Prepended_Concatenation_Mark`, "Total code points: 4174". Generated from
//! that file's `; Default_Ignorable_Code_Point` lines, not typed, and
//! checked against `regex-syntax` 0.8's independently generated
//! `ucd-16.0.0` table (the same 17 ranges once adjacent lines are merged).
//! The count and the entry count are pinned by a test, so a dropped or
//! widened range fails it. Regenerate from the next UCD's file rather than
//! editing by hand.

/// Sorted, disjoint, inclusive ranges. The comment on each is the UCD
/// line's general category and character name(s).
pub(super) const DEFAULT_IGNORABLE: &[(char, char)] = &[
    ('\u{00AD}', '\u{00AD}'),   // Cf: SOFT HYPHEN
    ('\u{034F}', '\u{034F}'),   // Mn: COMBINING GRAPHEME JOINER
    ('\u{061C}', '\u{061C}'),   // Cf: ARABIC LETTER MARK
    ('\u{115F}', '\u{1160}'),   // Lo: HANGUL CHOSEONG FILLER..HANGUL JUNGSEONG FILLER
    ('\u{17B4}', '\u{17B5}'),   // Mn: KHMER VOWEL INHERENT AQ..KHMER VOWEL INHERENT AA
    ('\u{180B}', '\u{180D}'),   // Mn: MONGOLIAN FREE VARIATION SELECTOR ONE..THREE
    ('\u{180E}', '\u{180E}'),   // Cf: MONGOLIAN VOWEL SEPARATOR
    ('\u{180F}', '\u{180F}'),   // Mn: MONGOLIAN FREE VARIATION SELECTOR FOUR
    ('\u{200B}', '\u{200F}'),   // Cf: ZERO WIDTH SPACE..RIGHT-TO-LEFT MARK
    ('\u{202A}', '\u{202E}'),   // Cf: LEFT-TO-RIGHT EMBEDDING..RIGHT-TO-LEFT OVERRIDE
    ('\u{2060}', '\u{2064}'),   // Cf: WORD JOINER..INVISIBLE PLUS
    ('\u{2065}', '\u{2065}'),   // Cn: <reserved-2065>
    ('\u{2066}', '\u{206F}'),   // Cf: LEFT-TO-RIGHT ISOLATE..NOMINAL DIGIT SHAPES
    ('\u{3164}', '\u{3164}'),   // Lo: HANGUL FILLER
    ('\u{FE00}', '\u{FE0F}'),   // Mn: VARIATION SELECTOR-1..VARIATION SELECTOR-16
    ('\u{FEFF}', '\u{FEFF}'),   // Cf: ZERO WIDTH NO-BREAK SPACE
    ('\u{FFA0}', '\u{FFA0}'),   // Lo: HALFWIDTH HANGUL FILLER
    ('\u{FFF0}', '\u{FFF8}'),   // Cn: <reserved-FFF0>..<reserved-FFF8>
    ('\u{1BCA0}', '\u{1BCA3}'), // Cf: SHORTHAND FORMAT LETTER OVERLAP..SHORTHAND FORMAT UP STEP
    ('\u{1D173}', '\u{1D17A}'), // Cf: MUSICAL SYMBOL BEGIN BEAM..MUSICAL SYMBOL END PHRASE
    ('\u{E0000}', '\u{E0000}'), // Cn: <reserved-E0000>
    ('\u{E0001}', '\u{E0001}'), // Cf: LANGUAGE TAG
    ('\u{E0002}', '\u{E001F}'), // Cn: <reserved-E0002>..<reserved-E001F>
    ('\u{E0020}', '\u{E007F}'), // Cf: TAG SPACE..CANCEL TAG
    ('\u{E0080}', '\u{E00FF}'), // Cn: <reserved-E0080>..<reserved-E00FF>
    ('\u{E0100}', '\u{E01EF}'), // Mn: VARIATION SELECTOR-17..VARIATION SELECTOR-256
    ('\u{E01F0}', '\u{E0FFF}'), // Cn: <reserved-E01F0>..<reserved-E0FFF>
];
