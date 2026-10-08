//! The refusal for an attribute name that a closed list does not contain
//! (`super::attribute_shape::check_shape`), for every position that has one.
//!
//! It names the attribute, the construct and the whole accepted list, adds
//! "did you mean" when a name is close, and says what the mistake would
//! have cost: an unread attribute has no effect, so a misspelled policy is
//! a missing authorization rule and a misspelled `@readonly` or
//! `@server_only` is a field left writable or exposed.

use super::attribute_shape::Known;
use super::misspelled_attributes::closest_name;

/// `a` or `an`, for the construct names in the messages (`a procedure`, `an
/// auth block field`).
pub(super) fn article(construct: &str) -> &'static str {
    if construct.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// "`x`", "`x` or `y`", "`x`, `y` or `z`".
fn join_or(names: &[String]) -> String {
    match names.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The names in `known` whose bare form is one of `bare`, quoted.
fn listed(known: &[Known], bare: &[&str]) -> Vec<String> {
    known
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| bare.contains(&name.trim_start_matches('@')))
        .map(|name| format!("`{name}`"))
        .collect()
}

/// What a misspelling of a name in `known` costs, when it is a protection.
fn consequence(known: &[Known]) -> String {
    let policies = listed(known, &["allow", "deny", "authorize"]);
    if !policies.is_empty() {
        return format!(
            " — for a misspelled {}, a silently missing authorization rule",
            join_or(&policies)
        );
    }
    let protections = listed(known, &["readonly", "server_only"]);
    if protections.is_empty() {
        return String::new();
    }
    format!(
        " — for a misspelled {}, a silently unprotected field",
        join_or(&protections)
    )
}

/// The reason text for `written`, which `known` does not list.
/// `elsewhere` are names that exist in the language but not in this list;
/// one of them is still offered as the likely meaning, with the note that
/// this construct does not take it.
pub(super) fn unsupported_attribute(
    written: &str,
    known: &[Known],
    elsewhere: &[&str],
    construct: &str,
) -> String {
    let a = article(construct);
    let sentence_a = if a == "an" { "An" } else { "A" };
    let hint = match closest_name(written, known.iter().map(|(name, _)| *name)) {
        Some(name) => format!(" (did you mean `{name}`?)"),
        // A name written exactly but accepted elsewhere needs no hint: the
        // refusal and the list say it all.
        None => closest_name(written, elsewhere.iter().copied())
            .filter(|name| *name != written)
            .map(|name| {
                format!(" (did you mean `{name}`? {sentence_a} {construct} does not take it)")
            })
            .unwrap_or_default(),
    };
    if known.is_empty() {
        return format!(
            "unsupported attribute `{written}` on {a} {construct}{hint}. {sentence_a} {construct} accepts \
             no attributes: nothing reads them, so one would have no effect",
        );
    }
    let list = known
        .iter()
        .map(|(name, _)| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "unsupported attribute `{written}` on {a} {construct}{hint}. {sentence_a} {construct} accepts \
         only {list}, spelled exactly so, and an attribute nothing reads would have no \
         effect{}",
        consequence(known),
    )
}
