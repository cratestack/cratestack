//! Cross-owner collision check for generated `@computed` resolver
//! method names.
//!
//! `cratestack-macros::computed::resolver_method_name` derives each
//! resolver's Rust method name as
//! `resolve_{snake(owner_name)}_{snake(field_name)}` — flattening the
//! owner/field boundary into a single snake_case identifier. Two
//! *different* (owner, field) pairs can flatten to the same string:
//! `model Image { setUrl String @computed }` and
//! `type ImageSet { url String @computed }` both produce
//! `resolve_image_set_url`. That is a duplicate trait method — a raw,
//! unhelpful rustc error (`E0201`) at the `include_*_schema!` call site
//! rather than a schema-authoring mistake caught at `cratestack check`
//! time. This module catches it here instead.
//!
//! This is distinct from
//! [`super::snake_case_collisions::validate_type_declaration_collisions`],
//! which already guards *owner-name* collisions (two `type`/`enum`/
//! `model` declarations normalizing to the same generated Rust type
//! name, which would also collide any `compose_<owner>_value` helper
//! derived from the owner name alone) — that check fires first and
//! would already reject `type Image` colliding with `model Image`.
//! What it does *not* catch is two distinct, non-colliding owner names
//! whose *fields* combine to the same flattened resolver name, which is
//! exactly the case above (`Image`/`ImageSet` don't collide with each
//! other).

use std::collections::BTreeMap;

use cratestack_core::route_naming::to_snake_case;
use cratestack_core::{Field, Schema, is_computed_field};

use crate::diagnostics::{SchemaError, span_error};

/// The exact resolver method name `cratestack-macros::computed::
/// resolver_method_name` would generate for this (owner, field) pair —
/// duplicated here rather than shared as a dependency, since
/// `cratestack-macros` depends on `cratestack-parser`'s validated
/// output, not the other way around; `to_snake_case` itself is the one
/// shared implementation both go through (`cratestack_core::route_naming`).
fn resolver_method_name(owner_name: &str, field_name: &str) -> String {
    format!(
        "resolve_{}_{}",
        to_snake_case(owner_name),
        to_snake_case(field_name)
    )
}

/// Rejects two `@computed` fields — on any mix of `type`/`model`
/// declarations — whose generated resolver method names collide.
pub(super) fn validate_computed_resolver_name_collisions(
    schema: &Schema,
) -> Result<(), SchemaError> {
    let owners = schema
        .models
        .iter()
        .map(|model| ("model", model.name.as_str(), &model.fields))
        .chain(
            schema
                .types
                .iter()
                .map(|ty| ("type", ty.name.as_str(), &ty.fields)),
        );

    let mut seen: BTreeMap<String, (&str, &str, &Field)> = BTreeMap::new();
    for (owner_kind, owner_name, fields) in owners {
        for field in fields.iter().filter(|field| is_computed_field(field)) {
            let resolver_name = resolver_method_name(owner_name, &field.name);
            if let Some((existing_kind, existing_owner, existing_field)) = seen.get(&resolver_name)
            {
                return Err(span_error(
                    format!(
                        "field `{}` on {} `{}` and field `{}` on {} `{}` both generate the \
                         resolver method `{}` after snake_case flattening (see \
                         `cratestack_core::route_naming::to_snake_case`) — rename one of the \
                         owners or one of the fields",
                        existing_field.name,
                        existing_kind,
                        existing_owner,
                        field.name,
                        owner_kind,
                        owner_name,
                        resolver_name,
                    ),
                    field.span,
                ));
            }
            seen.insert(resolver_name, (owner_kind, owner_name, field));
        }
    }

    Ok(())
}
