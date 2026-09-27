//! Rules 1 and 2 for the block-level `@@` attributes of models and views.
//!
//! A block-level attribute is its whole line, so a second attribute after
//! it on the same line is part of its raw text and nothing reads it. A
//! trailing `//` comment is not: the parser strips it, quote-aware, before
//! building the raw text (GHSA-69g4-xvcm-vm2j), so `@@allow("read", true)
//! // ask @ops` is `@@allow("read", true)` by the time it gets here.

use cratestack_core::Attribute;

use super::{scan, trailing_text};
use crate::diagnostics::{SchemaError, span_error};

/// Rule 1 for a block-level attribute of a `model` or `view`, and rule 2
/// for the names in `no_argument`.
pub(in crate::validate) fn validate_block_attribute_spelling(
    owner_kind: &str,
    owner_name: &str,
    attribute: &Attribute,
    no_argument: &[&str],
) -> Result<(), SchemaError> {
    let raw = attribute.raw.as_str();
    let offsets = scan::run_on_offsets(raw);
    if !offsets.is_empty() {
        return Err(span_error(
            format!(
                "{owner_kind} `{owner_name}` writes `{raw}` as one block attribute: a \
                 block-level attribute is its whole line, so what follows the first attribute \
                 is not recognised, and this is refused. Put each on a line of its own: `{}`",
                scan::separated(raw, &offsets, "` / `"),
            ),
            attribute.span,
        ));
    }
    let Some((name, rest)) = no_argument
        .iter()
        .find_map(|name| Some((*name, trailing_text(raw, name)?)))
    else {
        return Ok(());
    };
    let why = if rest.starts_with('(') {
        format!("`{name}` does not take arguments")
    } else {
        format!("`{name}` is recognised only when written exactly `{name}`")
    };
    Err(span_error(
        format!("{owner_kind} `{owner_name}` writes `{raw}`; {why}. It is refused: write `{name}`"),
        attribute.span,
    ))
}
