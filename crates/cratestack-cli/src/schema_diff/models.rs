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
/// `@@unique([...])` and `@@internal("action")` are the exception: a
/// model may carry several of either, each a distinct constraint/
/// suppressed action, so the argument is part of the identity. Keying
/// them all as `@@unique`/`@@internal` would collapse them into one
/// `BTreeMap` entry per model — `index_attributes` below only keeps the
/// last value written for a given key — and silently drop every
/// suppressed action but the last (cratestack#743 post-merge review,
/// Finding A: proven live — a model declaring both
/// `@@internal("create")` and `@@internal("update")` reported only the
/// `update` change, and a schema that dropped `@@internal("create")`
/// while keeping `@@internal("update")` reported **zero** changes at
/// all, which is precisely the class of defect this whole classification
/// exists to catch: a PR that restores a suppressed live action passing
/// the diff gate unnoticed).
///
/// The field/action text is whitespace-normalised before becoming part
/// of the key: `Attribute::raw` preserves the source line verbatim, so
/// `@@unique([a, b])`/`@@unique([a,b])` and `@@internal( "create" )`/
/// `@@internal("create")` are each the same constraint/action but would
/// otherwise produce different keys — reporting a cosmetic edit as a
/// remove-then-add rather than the value-only `Changed` every other
/// parenthesized attribute gets on a literal-text difference. Because
/// the action name is now baked into the key itself, two declarations of
/// the same action always collide onto one key regardless of which
/// order they're written in a schema, which is also what makes ordering
/// a non-issue for `@@internal` — `push_attribute_change`'s `Changed`
/// arm for `@@internal` can therefore only ever fire for a whitespace-
/// only edit of the same action (see its own doc for why that's
/// `Severity::Internal`, not `Breaking`).
fn attribute_key(raw: &str) -> String {
    if raw.starts_with("@@unique") || raw.starts_with("@@internal") {
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
/// *Removing* `@@internal(...)` is classified `Additive`: for the
/// ordinary case, restoring a route/arm/stub that was previously
/// suppressed breaks nobody's *existing, working* call — the same
/// "adding capability doesn't break anyone already using less of it"
/// reasoning applied to plain model/field additions elsewhere in this
/// module. This is not a universal guarantee, though: the design's own
/// motivating workflow (`docs/design/route-suppression.md`'s
/// `auth().isSystem()` case) is "disable the generated `create`, supply
/// a custom one" — if a consumer wrote that custom handler at the now-
/// freed path, removing `@@internal` reintroduces the generated route
/// at the same path, and the two collide at router-build time (axum
/// panics on duplicate route registration). A schema-only diff has no
/// way to see a consumer's out-of-schema custom handler, so it cannot
/// detect that case; `Additive` is still the right verdict for what
/// this tool can observe, but is not a claim that removal is always
/// safe.
///
/// A *value* change (`AttributeChange::Changed`) can only reach this
/// function for `@@internal` as a whitespace-only edit of the same
/// action — `attribute_key` now bakes the action name into the map key
/// (mirroring `@@unique`'s per-instance keying, cratestack#743
/// post-merge review Finding A), so two declarations naming different
/// actions are always a `Removed`+`Added` pair, never a same-key
/// `Changed`. A whitespace-only edit has no tracked wire-shape effect,
/// so it's `Internal`, not `Breaking` — there is no "which action lost
/// coverage" ambiguity left to be conservative about, because the
/// action is already part of the key.
fn push_attribute_change(
    changes: &mut Vec<Change>,
    model_name: &str,
    key: &str,
    change: AttributeChange,
) {
    let is_paged = key == "@@paged";
    let is_internal = key.starts_with("@@internal(");
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
            Severity::Internal,
            format!(
                "model `{model_name}` attribute changed from `{from}` to `{to}` — \
                 whitespace-only edit of the same suppressed action (a change to which \
                 action is suppressed always keys as a separate Added/Removed pair, never \
                 lands here; no tracked wire-shape effect)"
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
