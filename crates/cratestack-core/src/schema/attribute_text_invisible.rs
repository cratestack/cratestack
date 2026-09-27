//! Invisible characters in attribute text (GHSA-69g4-xvcm-vm2j,
//! maintainer decision 3: invisible characters refused, visible non-ASCII
//! text allowed).
//!
//! A character that displays as nothing is still part of the text every
//! reader compares. Inside an attribute name it turns `@@deny` into
//! another, unread name; inside a string it makes a policy literal
//! (`hasRole("ban\u{34F}ned")`) or a SQL body differ from what a reviewer
//! sees, so a `@deny` comparing against it never matches. Such a character
//! is refused anywhere in attribute text, strings included.
//!
//! # Which characters: `Default_Ignorable_Code_Point`
//!
//! The set is Unicode's `Default_Ignorable_Code_Point` property (DICP), the
//! characters a renderer is told to show as nothing when it does not
//! support them — exactly "renders as nothing" (the set was chosen under
//! the maintainer's standing secure-default rule). The table is generated
//! from the UCD 16.0 file (`attribute_text_invisible_table.rs` cites it and
//! a test pins its 4174 code points).
//!
//! An earlier version refused general category `Cf` instead, which is the
//! wrong set in both directions. It missed invisible characters that are
//! not `Cf` — the combining grapheme joiner U+034F, the Hangul fillers
//! U+115F, U+1160, U+3164 and U+FFA0, the Mongolian free variation
//! selectors U+180B–U+180D and U+180F, the Khmer inherent vowels
//! U+17B4/U+17B5, and the variation selectors — and each of those in a
//! `@deny(hasRole("ban…ned"))` let a caller with the role `banned` through
//! (measured: HTTP 200). And it refused visible characters that are `Cf`:
//! the Arabic number signs U+0600–U+0605, U+06DD, U+08E2 and the other
//! prepended concatenation marks (U+070F, U+0890, U+0891, U+110BD,
//! U+110CD), which DICP excludes because they display. They stay allowed.
//!
//! Four adjustments to the property:
//!
//! - **Variation selectors** (U+FE00–U+FE0F, U+E0100–U+E01EF) choose how
//!   the character before them is drawn (`❤` then U+FE0F is the emoji
//!   heart). **In a policy attribute** — `@allow`, `@deny`, `@authorize`,
//!   `@@allow`, `@@deny`, a query's `@allow`/`@deny`: any attribute whose
//!   name reads loosely as `allow`, `deny` or `authorize` — **one is
//!   refused wherever it stands** (decided under the maintainer's standing secure-default rule): the rule below
//!   let `hasRole("banné\u{FE0F}")` or `hasRole("管理者\u{FE0F}")` through,
//!   which renders like the plain role, yet a caller whose role is `banné`
//!   passes the deny. A role, an action or an expression has no use for a
//!   presentation choice. **Elsewhere** (an emoji in a `@default`, say) one
//!   is allowed right after a visible non-ASCII character: not ASCII, not
//!   whitespace, not itself invisible (so not after another selector).
//!   After an ASCII character (`n\u{FE0F}` draws as `n`), after whitespace
//!   or at the start it changes nothing on screen and is refused. A keycap
//!   emoji (a digit then U+FE0F) is refused by this rule.
//! - **U+2800 BRAILLE PATTERN BLANK and U+1D159 MUSICAL SYMBOL NULL
//!   NOTEHEAD** are not DICP, as each takes up a cell, but each draws as
//!   blank and is not whitespace either, so `"ban\u{2800}ned"` reads as
//!   `"ban ned"` to a reviewer and as neither to the comparison. Both are
//!   refused (U+1D159 decided under the maintainer's standing secure-default rule, for the reason U+2800 is).
//! - **The format controls DICP's derivation subtracts by name** are
//!   refused as well: U+FFF9–U+FFFB (interlinear annotation anchor,
//!   separator, terminator) and U+13430–U+1343F (the Egyptian hieroglyph
//!   format controls). All nineteen are `Cf` with no glyph of their own;
//!   the property leaves them out by name (`- FFF9..FFFB - 13430..13440`),
//!   so a renderer without support is not told to hide them, and whether
//!   one is seen depends on the viewer — an editor, a terminal, a review
//!   page — while the comparison sees another string. U+13440, the last
//!   code point of that range, is a combining mark (`Mn`), not a format
//!   control, and is not added. With these, the only `Cf` characters
//!   allowed are the prepended concatenation marks above.
//! - **Control characters** (general category `Cc`) that are not
//!   whitespace are refused too. DICP leaves out `Cc` entirely, but none
//!   has a glyph: whether NUL, BEL or a C1 control such as U+0080 shows at
//!   all depends on the viewer, and ESC starts a terminal escape sequence
//!   that can conceal the text after it (SGR 8). `hasRole("ban\u{80}ned")`
//!   checked `schema OK` before this. Tab, line feed and carriage return
//!   stay allowed here; the vertical tab, form feed and NEL are refused by
//!   `cratestack-parser` (`parse::hidden_breaks`) wherever text follows.
//!
//! The zero-width joiner and non-joiner (U+200D, U+200C) stay refused
//! (maintainer decision: zero-width characters are refused even where a
//! script or an emoji sequence uses them), strings included.
//!
//! A diagnostic that quotes attribute text escapes these characters rather
//! than writing them raw ([`escape_for_diagnostic`], its module doc).

#[path = "attribute_text_invisible_table.rs"]
mod table;

#[path = "attribute_text_invisible_describe.rs"]
mod describe;

#[path = "attribute_text_invisible_escape.rs"]
mod escape;

pub use describe::describe_invisible_character;
pub use escape::{escape_for_diagnostic, substitute_for_display};
use table::DEFAULT_IGNORABLE;

/// U+2800, refused although it is not default-ignorable (module doc).
const BRAILLE_PATTERN_BLANK: char = '\u{2800}';

/// U+1D159, refused like U+2800: it draws as blank (module doc).
const MUSICAL_SYMBOL_NULL_NOTEHEAD: char = '\u{1D159}';

/// The `Cf` format controls DICP's derivation subtracts by name, refused
/// although they are not default-ignorable (module doc).
const INVISIBLE_FORMAT_CONTROLS: [(char, char); 2] = [
    ('\u{FFF9}', '\u{FFFB}'),   // INTERLINEAR ANNOTATION ANCHOR..TERMINATOR
    ('\u{13430}', '\u{1343F}'), // EGYPTIAN HIEROGLYPH VERTICAL JOINER..END WALLED ENCLOSURE
];

/// Attribute names, read loosely, whose text is a policy: in one, a
/// variation selector is refused wherever it stands (module doc).
const POLICY_ATTRIBUTE_NAMES: [&str; 3] = ["allow", "deny", "authorize"];

/// Whether `ch` is refused wherever it stands: default-ignorable, U+2800,
/// U+1D159, one of [`INVISIBLE_FORMAT_CONTROLS`], or a control character
/// that is not whitespace (module doc).
fn is_always_invisible(ch: char) -> bool {
    is_default_ignorable(ch)
        || ch == BRAILLE_PATTERN_BLANK
        || ch == MUSICAL_SYMBOL_NULL_NOTEHEAD
        || (ch.is_control() && !ch.is_whitespace())
        || INVISIBLE_FORMAT_CONTROLS
            .iter()
            .any(|&(first, last)| (first..=last).contains(&ch))
}

/// Whether `ch` is a Unicode 16.0 `Default_Ignorable_Code_Point`.
pub fn is_default_ignorable(ch: char) -> bool {
    DEFAULT_IGNORABLE
        .binary_search_by(|&(first, last)| {
            if last < ch {
                std::cmp::Ordering::Less
            } else if first > ch {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Whether `ch` is a variation selector (U+FE00–U+FE0F, U+E0100–U+E01EF).
fn is_variation_selector(ch: char) -> bool {
    matches!(ch, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Whether a variation selector may follow `base` outside a policy
/// attribute: a visible non-ASCII character, one that is drawn and can
/// have a presentation to vary.
fn takes_variation(base: char) -> bool {
    !base.is_ascii() && !base.is_whitespace() && !is_always_invisible(base)
}

/// Whether any attribute name in `raw`, read loosely
/// ([`super::loose_attribute_names`]: any case, invisible and punctuation
/// characters dropped), is `allow`, `deny` or `authorize`.
pub fn is_policy_attribute(raw: &str) -> bool {
    super::loose_attribute_names(raw)
        .iter()
        .any(|name| POLICY_ATTRIBUTE_NAMES.contains(&name.as_str()))
}

/// The byte offset and value of the first invisible character in the
/// attribute text `raw`, inside a string literal or not. Whether `raw` is
/// a policy attribute ([`is_policy_attribute`]) is read from `raw` itself.
pub fn invisible_character(raw: &str) -> Option<(usize, char)> {
    first_invisible(raw, is_policy_attribute(raw))
}

/// [`invisible_character`] for text the caller has read as a policy
/// attribute itself: every variation selector is refused.
pub fn invisible_character_in_policy(raw: &str) -> Option<(usize, char)> {
    first_invisible(raw, true)
}

fn first_invisible(raw: &str, policy: bool) -> Option<(usize, char)> {
    let mut previous = None;
    for (index, ch) in raw.char_indices() {
        let refused = if is_variation_selector(ch) {
            policy || !previous.is_some_and(takes_variation)
        } else {
            is_always_invisible(ch)
        };
        if refused {
            return Some((index, ch));
        }
        previous = Some(ch);
    }
    None
}

#[cfg(test)]
#[path = "attribute_text_invisible_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "attribute_text_invisible_control_tests.rs"]
mod control_tests;

#[cfg(test)]
#[path = "attribute_text_invisible_policy_tests.rs"]
mod policy_tests;
