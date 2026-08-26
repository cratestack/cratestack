use std::collections::BTreeMap;

use cratestack_core::{Model, Schema};

use super::fields::diff_fields;
use super::{Change, Severity};

pub(super) fn diff_models(prev: &Schema, next: &Schema, changes: &mut Vec<Change>) {
    let prev_by_name = index(&prev.models);
    let next_by_name = index(&next.models);

    for name in prev_by_name.keys() {
        if !next_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Breaking,
                category: "model_removed",
                subject: format!("model `{name}`"),
                message: format!("model `{name}` was removed"),
            });
        }
    }

    for name in next_by_name.keys() {
        if !prev_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Additive,
                category: "model_added",
                subject: format!("model `{name}`"),
                message: format!("model `{name}` was added"),
            });
        }
    }

    for (name, prev_model) in &prev_by_name {
        let Some(next_model) = next_by_name.get(name) else {
            continue;
        };
        diff_attributes(name, prev_model, next_model, changes);
        diff_fields(name, &prev_model.fields, &next_model.fields, changes);
    }
}

fn index(models: &[Model]) -> BTreeMap<&str, &Model> {
    models
        .iter()
        .map(|model| (model.name.as_str(), model))
        .collect()
}

/// The attribute's identity ignoring any parenthesized arguments, e.g.
/// `@@retain(days: 5)` and `@@retain(days: 10)` share the key
/// `@@retain` — a value-only change, not an add/remove.
///
/// `@@unique([...])` is the exception: a model may carry several, each
/// a distinct constraint, so the argument list is part of the identity.
/// Keying them all as `@@unique` would collapse them into one entry and
/// under-report adds and removals.
///
/// The field list is whitespace-normalised before becoming part of the
/// key: `Attribute::raw` preserves the source line verbatim, so
/// `@@unique([a, b])` and `@@unique([a,b])` are the same constraint but
/// would otherwise produce different keys — reporting a cosmetic edit
/// as a remove-then-add rather than the value-only `Changed` every
/// other parenthesized attribute gets on a literal-text difference.
fn attribute_key(raw: &str) -> String {
    if raw.starts_with("@@unique") {
        return raw.chars().filter(|ch| !ch.is_ascii_whitespace()).collect();
    }
    raw.split('(').next().unwrap_or(raw).to_owned()
}

fn diff_attributes(model_name: &str, prev: &Model, next: &Model, changes: &mut Vec<Change>) {
    let prev_by_key = index_attributes(&prev.attributes);
    let next_by_key = index_attributes(&next.attributes);

    for (key, raw) in &prev_by_key {
        match next_by_key.get(key) {
            None => push_attribute_change(changes, model_name, key, AttributeChange::Removed(raw)),
            Some(next_raw) if next_raw != raw => push_attribute_change(
                changes,
                model_name,
                key,
                AttributeChange::Changed(raw, next_raw),
            ),
            _ => {}
        }
    }

    for (key, raw) in &next_by_key {
        if !prev_by_key.contains_key(key) {
            push_attribute_change(changes, model_name, key, AttributeChange::Added(raw));
        }
    }
}

fn index_attributes(attributes: &[cratestack_core::Attribute]) -> BTreeMap<String, &str> {
    attributes
        .iter()
        .map(|attribute| (attribute_key(&attribute.raw), attribute.raw.as_str()))
        .collect()
}

enum AttributeChange<'a> {
    Added(&'a str),
    Removed(&'a str),
    Changed(&'a str, &'a str),
}

/// Classifies a model-attribute change. `@@paged` is the one case the
/// issue explicitly calls out as wire-breaking (it swaps `.list()`'s
/// response envelope between `T[]` and `Page<T>`); `@@internal(...)`
/// (cratestack#743, `docs/design/route-suppression.md`) is the other:
/// *adding* it deletes a live REST route / RPC dispatch arm / client
/// stub out from under any existing consumer calling that action —
/// exactly the class of change `model_removed` above already treats
/// as `Breaking` for a whole model, just scoped to one action. Every
/// other model-level attribute (`@@soft_delete`, `@@audit`,
/// `@@retain`, `@@emit`) affects server behavior but not the shape of
/// the wire contract this tool tracks, so it's reported as
/// internal-only — a documented scope gap, not an oversight.
///
/// *Removing* `@@internal(...)` is classified `Additive`, not
/// `Breaking`: it restores a route/arm/stub that was previously
/// suppressed, so no existing consumer's working call becomes invalid
/// — the same "adding capability doesn't break anyone already using
/// less of it" reasoning applied to plain model/field additions
/// elsewhere in this module. A *value* change (e.g. `@@internal
/// ("create")` → `@@internal("update")`) is conservatively classified
/// `Breaking` too: the diff only has the two raw attribute strings to
/// compare, not the parsed action sets, so it can't cheaply prove no
/// action lost suppression coverage — treating it as breaking is the
/// fail-safe direction (a false-positive gate failure a human can
/// wave through beats a false-negative that ships a silent route
/// removal).
fn push_attribute_change(
    changes: &mut Vec<Change>,
    model_name: &str,
    key: &str,
    change: AttributeChange,
) {
    let is_paged = key == "@@paged";
    let is_internal = key == "@@internal";
    let (severity, message) = match change {
        AttributeChange::Added(raw) if is_paged => (
            Severity::Breaking,
            format!(
                "model `{model_name}` gained `{raw}` — `{model_name}.list()`'s response \
                 envelope changes from `{model_name}[]` to `Page<{model_name}>`"
            ),
        ),
        AttributeChange::Removed(raw) if is_paged => (
            Severity::Breaking,
            format!(
                "model `{model_name}` lost `{raw}` — `{model_name}.list()`'s response \
                 envelope changes from `Page<{model_name}>` back to `{model_name}[]`"
            ),
        ),
        AttributeChange::Added(raw) if is_internal => (
            Severity::Breaking,
            format!(
                "model `{model_name}` gained `{raw}` — the suppressed action's REST route, \
                 RPC dispatch arm, and client stub are all removed for any consumer still \
                 calling it"
            ),
        ),
        AttributeChange::Removed(raw) if is_internal => (
            Severity::Additive,
            format!(
                "model `{model_name}` lost `{raw}` — the previously suppressed action's \
                 route/arm/stub is restored"
            ),
        ),
        AttributeChange::Changed(from, to) if is_internal => (
            Severity::Breaking,
            format!(
                "model `{model_name}` attribute changed from `{from}` to `{to}` — \
                 conservatively treated as breaking since which action(s) lost suppression \
                 coverage can't be determined from the raw attribute text alone"
            ),
        ),
        AttributeChange::Added(raw) => (
            Severity::Internal,
            format!("model `{model_name}` gained `{raw}` (no tracked wire-shape effect)"),
        ),
        AttributeChange::Removed(raw) => (
            Severity::Internal,
            format!("model `{model_name}` lost `{raw}` (no tracked wire-shape effect)"),
        ),
        AttributeChange::Changed(from, to) => (
            Severity::Internal,
            format!(
                "model `{model_name}` attribute changed from `{from}` to `{to}` \
                 (no tracked wire-shape effect)"
            ),
        ),
    };
    changes.push(Change {
        severity,
        category: if is_paged {
            "model_attribute_paged"
        } else if is_internal {
            "model_attribute_internal"
        } else {
            "model_attribute_other"
        },
        subject: format!("model `{model_name}`"),
        message,
    });
}
