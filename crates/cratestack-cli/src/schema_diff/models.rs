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
fn attribute_key(raw: &str) -> &str {
    raw.split('(').next().unwrap_or(raw)
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

fn index_attributes(attributes: &[cratestack_core::Attribute]) -> BTreeMap<&str, &str> {
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
/// response envelope between `T[]` and `Page<T>`); every other
/// model-level attribute (`@@soft_delete`, `@@audit`, `@@retain`,
/// `@@emit`) affects server behavior but not the shape of the wire
/// contract this tool tracks, so it's reported as internal-only — a
/// documented scope gap, not an oversight.
fn push_attribute_change(
    changes: &mut Vec<Change>,
    model_name: &str,
    key: &str,
    change: AttributeChange,
) {
    let is_paged = key == "@@paged";
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
        } else {
            "model_attribute_other"
        },
        subject: format!("model `{model_name}`"),
        message,
    });
}
