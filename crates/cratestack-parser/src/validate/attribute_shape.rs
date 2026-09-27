//! A closed list of attributes, each in the one spelling its reader takes
//! (GHSA-69g4-xvcm-vm2j), for the constructs whose attributes carry
//! authorization: `procedure` and `query`.
//!
//! The generators recognise these attributes by exact text — `@deny(`
//! with nothing between the name and the `(`, a line that ends at the
//! closing `)` — and silently skip anything else. So `@deny (x)`,
//! `@Deny(x)`, `@deyn(x)`, `@deny(x);` or `@deny(x) banned` passed
//! `cratestack check` and generated a procedure with no deny rule. Here
//! every attribute must be a known name, spelled exactly, with an
//! argument list exactly when the name takes one, and nothing after it.
//! The rules match the field-attribute ones in `super::attribute_spelling`.

use cratestack_core::Attribute;
use cratestack_core::schema::attribute_text::{attribute_starts, group_end};

use super::attribute_spelling::scan::separated;
use super::misspelled_attributes::optimal_string_alignment;
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
        let list = known
            .iter()
            .map(|(name, _)| format!("`{name}`"))
            .collect::<Vec<_>>();
        let hint = suggestion(written, known)
            .map(|name| format!(" (did you mean `{name}`?)"))
            .unwrap_or_default();
        let policies = known
            .iter()
            .map(|(name, _)| *name)
            .filter(|name| matches!(name.trim_start_matches('@'), "allow" | "deny" | "authorize"))
            .map(|name| format!("`{name}`"))
            .collect::<Vec<_>>();
        let policies = match policies.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
            None => "policy attribute".to_owned(),
        };
        return refuse(format!(
            "unsupported attribute `{written}` on a {construct}{hint}. A {construct} accepts only \
             {}, spelled exactly so, and an attribute nothing reads would have no effect — \
             for a misspelled {policies}, a silently missing authorization rule",
            list.join(", ")
        ));
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
        return refuse(format!(
            "the argument list of `{name}` is never closed on this line (a {construct}'s \
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

/// The known name `written` most likely means: the same name in another
/// case or with other sigils, or one a typo away.
fn suggestion(written: &str, known: &[Known]) -> Option<&'static str> {
    let bare = written.trim_start_matches('@').to_ascii_lowercase();
    if bare.chars().count() < 3 {
        return None;
    }
    let limit = if bare.chars().count() <= 5 { 1 } else { 2 };
    known
        .iter()
        .map(|(name, _)| {
            let distance = optimal_string_alignment(&bare, name.trim_start_matches('@'));
            (*name, distance)
        })
        .filter(|(_, distance)| *distance <= limit)
        .min_by_key(|(name, distance)| (*distance, name.len()))
        .map(|(name, _)| name)
}
