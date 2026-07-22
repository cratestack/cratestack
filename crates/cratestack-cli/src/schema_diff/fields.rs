use std::collections::BTreeMap;

use cratestack_core::{Field, TypeArity};

use super::arity::{arity_label, classify_arity_change};
use super::{Change, Severity};

pub(super) fn diff_fields(
    model_name: &str,
    prev: &[Field],
    next: &[Field],
    changes: &mut Vec<Change>,
) {
    let prev_by_name = index(prev);
    let next_by_name = index(next);

    for name in prev_by_name.keys() {
        if !next_by_name.contains_key(name) {
            changes.push(Change {
                severity: Severity::Breaking,
                category: "field_removed",
                subject: format!("{model_name}.{name}"),
                message: format!("field `{model_name}.{name}` was removed"),
            });
        }
    }

    for (name, next_field) in &next_by_name {
        match prev_by_name.get(name) {
            None => push_added_field(changes, model_name, name, next_field),
            Some(prev_field) => {
                diff_matched_field(model_name, name, prev_field, next_field, changes)
            }
        }
    }
}

fn index(fields: &[Field]) -> BTreeMap<&str, &Field> {
    fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect()
}

fn push_added_field(changes: &mut Vec<Change>, model_name: &str, name: &str, field: &Field) {
    let breaks_construction =
        field.ty.arity == TypeArity::Required && !has_default(field) && !is_server_only(field);

    if breaks_construction {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "field_added_required",
            subject: format!("{model_name}.{name}"),
            message: format!(
                "field `{model_name}.{name}` was added as a required field with no default \
                 — existing client payloads that omit it will be rejected"
            ),
        });
        return;
    }

    changes.push(Change {
        severity: Severity::Additive,
        category: "field_added",
        subject: format!("{model_name}.{name}"),
        message: format!("field `{model_name}.{name}` was added"),
    });
}

fn diff_matched_field(
    model_name: &str,
    name: &str,
    prev: &Field,
    next: &Field,
    changes: &mut Vec<Change>,
) {
    if prev.ty.name != next.ty.name {
        changes.push(Change {
            severity: Severity::Breaking,
            category: "field_retyped",
            subject: format!("{model_name}.{name}"),
            message: format!(
                "field `{model_name}.{name}` type changed from `{}` to `{}`",
                prev.ty.name, next.ty.name
            ),
        });
        return;
    }

    if prev.ty.arity != next.ty.arity {
        let severity = classify_arity_change(prev.ty.arity, next.ty.arity);
        changes.push(Change {
            severity,
            category: "field_arity_changed",
            subject: format!("{model_name}.{name}"),
            message: format!(
                "field `{model_name}.{name}` arity changed from {} to {}",
                arity_label(prev.ty.arity),
                arity_label(next.ty.arity)
            ),
        });
    }
}

fn has_default(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw.starts_with("@default"))
}

fn is_server_only(field: &Field) -> bool {
    field
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@server_only")
}
