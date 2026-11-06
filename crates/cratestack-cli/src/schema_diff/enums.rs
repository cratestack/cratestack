//! Enums, matched by name. A string enum's variants are matched by name,
//! since the name is the wire value. An ordinal enum's (`Pending = 1`,
//! `docs/design/enum-ordinals.md` §3.3) are matched by ordinal, since the
//! number is: there, a rename moves nothing on the wire.

use std::collections::BTreeMap;

use cratestack_core::{EnumDecl, Schema};

use super::response_reach::response_reachable;
use super::{Change, Severity};

pub(super) fn diff_enums(prev: &Schema, next: &Schema, changes: &mut Vec<Change>) {
    let prev_by_name = index(prev);
    let next_by_name = index(next);
    let in_responses = response_reachable(next);

    for name in prev_by_name.keys() {
        if !next_by_name.contains_key(name) {
            changes.push(change(
                Severity::Breaking,
                "enum_removed",
                name,
                format!("enum `{name}` was removed"),
            ));
        }
    }
    for name in next_by_name.keys() {
        if !prev_by_name.contains_key(name) {
            changes.push(change(
                Severity::Additive,
                "enum_added",
                name,
                format!("enum `{name}` was added"),
            ));
        }
    }
    for (name, was) in &prev_by_name {
        let Some(now) = next_by_name.get(name) else {
            continue;
        };
        let decoded = in_responses.contains(name);
        match (is_ordinal(was), is_ordinal(now)) {
            (false, false) => by_name(name, was, now, decoded, changes),
            (true, true) => by_ordinal(name, was, now, decoded, changes),
            (was_ordinal, _) => changes.push(change(
                Severity::Breaking,
                "enum_encoding_changed",
                name,
                format!(
                    "enum `{name}` changed from {} to {} on the wire; every client must be \
                     regenerated",
                    encoding(was_ordinal),
                    encoding(!was_ordinal)
                ),
            )),
        }
    }
}

fn index(schema: &Schema) -> BTreeMap<&str, &EnumDecl> {
    schema.enums.iter().map(|e| (e.name.as_str(), e)).collect()
}

fn is_ordinal(e: &EnumDecl) -> bool {
    e.variants.iter().any(|v| v.ordinal.is_some())
}

fn encoding(ordinal: bool) -> &'static str {
    if ordinal { "ordinals" } else { "names" }
}

fn by_name(name: &str, was: &EnumDecl, now: &EnumDecl, decoded: bool, changes: &mut Vec<Change>) {
    let (before, after) = (names(was), names(now));
    for variant in before.iter().filter(|v| !after.contains(v)) {
        changes.push(change(
            Severity::Breaking,
            "enum_variant_removed",
            name,
            format!("variant `{variant}` was removed from enum `{name}`"),
        ));
    }
    for variant in after.iter().filter(|v| !before.contains(v)) {
        changes.push(added_variant(name, &format!("`{variant}`"), decoded));
    }
    if common_order(&before, &after) != common_order(&after, &before) {
        changes.push(change(
            Severity::Internal,
            "enum_variants_reordered",
            name,
            format!(
                "variants of enum `{name}` were reordered; the wire carries names, so only \
                 declaration order (and the first variant, the generated `Default`) changed"
            ),
        ));
    }
}

fn by_ordinal(
    name: &str,
    was: &EnumDecl,
    now: &EnumDecl,
    decoded: bool,
    changes: &mut Vec<Change>,
) {
    let (before, after) = (numbered(was), numbered(now));
    for (ordinal, variant) in &before {
        match after.get(ordinal) {
            None => changes.push(change(
                Severity::Breaking,
                "enum_variant_removed",
                name,
                format!("variant `{variant}` (ordinal {ordinal}) was removed from enum `{name}`"),
            )),
            Some(renamed) if renamed != variant => changes.push(change(
                Severity::Breaking,
                "enum_variant_renamed",
                name,
                format!(
                    "variant `{variant}` of enum `{name}` was renamed to `{renamed}`; the wire is \
                     unchanged (ordinal {ordinal}), but generated client identifiers change"
                ),
            )),
            Some(_) => {}
        }
    }
    for (ordinal, variant) in after.iter().filter(|(o, _)| !before.contains_key(o)) {
        changes.push(added_variant(
            name,
            &format!("`{variant}` (ordinal {ordinal})"),
            decoded,
        ));
    }
}

fn names(e: &EnumDecl) -> Vec<&str> {
    e.variants.iter().map(|v| v.name.as_str()).collect()
}

/// `list`'s variants that `other` also has, in `list`'s order.
fn common_order<'a>(list: &[&'a str], other: &[&str]) -> Vec<&'a str> {
    list.iter().copied().filter(|v| other.contains(v)).collect()
}

fn numbered(e: &EnumDecl) -> BTreeMap<i64, &str> {
    e.variants
        .iter()
        .filter_map(|v| Some((v.ordinal?, v.name.as_str())))
        .collect()
}

/// A new value is harmless in a request and breaking in a response: a
/// client generated from the old schema refuses a value it does not know
/// (the Dart and Rust decoders do).
fn added_variant(name: &str, variant: &str, decoded: bool) -> Change {
    if decoded {
        change(
            Severity::Breaking,
            "enum_variant_added",
            name,
            format!(
                "variant {variant} was added to enum `{name}`, which responses carry; a client \
                 generated from the old schema refuses the new value"
            ),
        )
    } else {
        change(
            Severity::Additive,
            "enum_variant_added",
            name,
            format!("variant {variant} was added to enum `{name}`, which only requests carry"),
        )
    }
}

fn change(severity: Severity, category: &'static str, name: &str, message: String) -> Change {
    Change {
        severity,
        category,
        subject: format!("enum `{name}`"),
        message,
    }
}
