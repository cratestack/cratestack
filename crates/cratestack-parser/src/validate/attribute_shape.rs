//! A closed list of attributes, each in the one spelling its reader takes
//! (GHSA-69g4-xvcm-vm2j): the attributes of a `procedure` and a `query`,
//! the `@@` attributes of a `model` and a `view`, and, since ADR 0019 D5,
//! the field attributes of every field-bearing declaration.
//!
//! The generators recognise these attributes by exact text — `@deny(`
//! with nothing between the name and the `(`, a line that ends at the
//! closing `)` — and silently skip anything else. So `@deny (x)`,
//! `@Deny(x)`, `@deyn(x)`, `@deny(x);` or `@deny(x) banned` passed
//! `cratestack check` and generated a procedure with no deny rule. Here
//! every attribute must be a known name, spelled exactly, with an
//! argument list exactly when the name takes one, and nothing after it.
//! A name outside the list is refused by `super::unsupported_attribute`,
//! whose suggestion comes from `super::misspelled_attributes`.

use cratestack_core::Attribute;
use cratestack_core::schema::attribute_text::{attribute_starts, group_end};

use super::attribute_spelling::scan::separated;
use super::unsupported_attribute::{article, unsupported_attribute};
use crate::diagnostics::{SchemaError, span_error};

/// Whether an attribute takes an argument list.
#[derive(Clone, Copy)]
pub(super) enum Arguments {
    /// Written bare: `@stream`.
    None,
    /// Written with a non-empty list: `@deny(expr)`.
    Required,
    /// Either: `@deprecated` or `@deprecated("why")`.
    Optional,
}

/// A known attribute, sigils included (`@deny`, `@@sql`).
pub(super) type Known = (&'static str, Arguments);

/// Checks `attribute` against `known`, for `owner` (``procedure `x` ``)
/// of kind `construct` (`"procedure"`). Returns the matched name and the
/// text inside its argument list, if it has one.
pub(super) fn check_shape<'a>(
    attribute: &'a Attribute,
    known: &[Known],
    owner: &str,
    construct: &str,
) -> Result<(&'static str, Option<&'a str>), SchemaError> {
    check_shape_hinted(attribute, known, &[], owner, construct)
}

/// [`check_shape`], whose "did you mean" may also name an attribute that
/// `known` does not list but `elsewhere` does: a typo of `@readonly` on a
/// `type` field is pointed at `@readonly` even though a `type` does not
/// accept it, which the refusal then says.
pub(super) fn check_shape_hinted<'a>(
    attribute: &'a Attribute,
    known: &[Known],
    elsewhere: &[&str],
    owner: &str,
    construct: &str,
) -> Result<(&'static str, Option<&'a str>), SchemaError> {
    let raw = attribute.raw.as_str();
    let refuse = |why: String| {
        Err(span_error(
            format!("{owner} writes `{raw}`: {why}. It is refused"),
            attribute.span,
        ))
    };
    let run_on = attribute_starts(raw);
    if !run_on.is_empty() {
        return refuse(format!(
            "attributes with no space between them are read as one unrecognised attribute, \
             so none of them has any effect. Separate them with a space: `{}`",
            separated(raw, &run_on, " ")
        ));
    }
    let sigils = raw.len() - raw.trim_start_matches('@').len();
    let after = &raw[sigils..];
    if after.starts_with(char::is_whitespace) {
        return refuse(format!(
            "no space is allowed between `{}` and the attribute name",
            &raw[..sigils]
        ));
    }
    let name_len = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(after.len());
    let written = &raw[..sigils + name_len];
    let Some(&(name, arguments)) = known.iter().find(|(name, _)| *name == written) else {
        return refuse(unsupported_attribute(written, known, elsewhere, construct));
    };
    let rest = &raw[written.len()..];
    let expects_list = !matches!(arguments, Arguments::None);
    if rest.is_empty() {
        return match arguments {
            Arguments::Required => refuse(format!("`{name}` takes an argument list: `{name}(…)`")),
            _ => Ok((name, None)),
        };
    }
    if !rest.starts_with('(') {
        return refuse(if expects_list && rest.trim_start().starts_with('(') {
            format!(
                "`{name}` must be followed directly by its `(`: a generator reads `{name}(`, \
                 so with anything between them the attribute has no effect"
            )
        } else {
            format!(
                "`{rest}` after `{name}` is not part of any attribute, and a generator reads \
                 `{name}` only when nothing follows it. Remove it, or make it a `//` comment"
            )
        });
    }
    if !expects_list {
        return refuse(format!(
            "`{name}` does not take arguments, and a generator recognises it only when written \
             exactly `{name}`"
        ));
    }
    let Some(end) = group_end(raw, written.len()) else {
        let a = article(construct);
        return refuse(format!(
            "the argument list of `{name}` is never closed on this line ({a} {construct}'s \
             attribute must fit on one line)"
        ));
    };
    let tail = raw[end..].trim();
    if !tail.is_empty() {
        return refuse(format!(
            "`{tail}` after the closing `)` of `{name}` is not part of any attribute, and a \
             generator reads `{name}(…)` only when the line ends at that `)`. Remove it, or \
             make it a `//` comment"
        ));
    }
    let inner = raw[written.len() + 1..end - 1].trim();
    if inner.is_empty() {
        return refuse(format!("`{name}()` has an empty argument list"));
    }
    Ok((name, Some(inner)))
}
