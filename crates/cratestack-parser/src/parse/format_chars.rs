//! Invisible characters in attribute text are refused
//! (GHSA-69g4-xvcm-vm2j, maintainer decision 3).
//!
//! Applied to every attribute as the parser builds it — field attributes,
//! a model's or view's `@@…` lines (a multi-line `"""` SQL body included),
//! and a procedure's or query's attribute run — so the check sees each
//! attribute's whole text, strings included, and nothing a trailing `//`
//! comment holds. Which characters count — Unicode's
//! `Default_Ignorable_Code_Point`, any variation selector in a policy
//! attribute (`@allow`, `@deny`, `@authorize`, `@@allow`, `@@deny`) and
//! elsewhere one not right after a visible non-ASCII character, U+2800,
//! U+1D159, the format controls U+FFF9–U+FFFB and U+13430–U+1343F, and
//! control characters that are not whitespace — is
//! [`cratestack_core::schema::attribute_text::invisible_character`]. The
//! error quotes the attribute with those characters escaped
//! (`SchemaError::new`).

use cratestack_core::Attribute;
use cratestack_core::schema::attribute_text::{describe_invisible_character, invisible_character};

use crate::diagnostics::SchemaError;
use crate::line_helpers::{Line, joined_offset_in_source};

/// Refuses the first of `attributes` that carries an invisible character.
/// `lines` are the source lines the attributes were read from.
pub(super) fn refuse_invisible_characters(
    attributes: &[Attribute],
    lines: &[Line<'_>],
) -> Result<(), SchemaError> {
    for attribute in attributes {
        let Some((offset, ch)) = invisible_character(&attribute.raw) else {
            continue;
        };
        let (start, number, column) = locate(attribute, offset, lines);
        return Err(SchemaError::new(
            format!(
                "`{}` contains {} at line {number}, column {column}: an invisible \
                 character, which makes the attribute read differently from how it displays. \
                 In a name it makes another attribute that nothing reads, in a string it \
                 changes the value a policy or SQL body compares. Remove it; visible \
                 non-ASCII text such as `é` or `中` is fine, and so is an emoji's \
                 variation selector outside a policy attribute",
                attribute.raw,
                describe_invisible_character(ch)
            ),
            start..start + ch.len_utf8(),
            number,
        ));
    }
    Ok(())
}

/// Source byte offset, line number and 1-based column (in characters) of
/// byte `offset` of `attribute.raw`, which may span several of `lines`.
fn locate(attribute: &Attribute, offset: usize, lines: &[Line<'_>]) -> (usize, usize, usize) {
    let fallback = (attribute.span.start, attribute.span.line, 1);
    let Some(first) = lines
        .iter()
        .position(|line| line.number == attribute.span.line)
    else {
        return fallback;
    };
    let lead = attribute.span.start - lines[first].start;
    let (start, number) = joined_offset_in_source(&lines[first..], lead + offset);
    let column = lines
        .iter()
        .find(|line| line.number == number)
        .and_then(|line| line.raw.get(..start - line.start))
        .map_or(1, |before| before.chars().count() + 1);
    (start, number, column)
}
