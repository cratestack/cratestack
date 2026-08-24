//! Schema-wide `@computed` "bearing" analysis for codegen — mirrors
//! `cratestack_parser::validate::computed::computed_bearing_names`
//! bit-for-bit (see that module's doc for the algorithm itself and why
//! it needs the *whole* schema rather than one declaration at a time).
//! The parser already rejects any schema this crate would disagree
//! with (a computed field's own type being computed-bearing, a
//! computed-bearing procedure argument, a `@stream` item that's
//! computed-bearing), so this is a codegen-side re-derivation of the
//! same fixed point, not a second source of truth for what's legal —
//! duplicated rather than shared as a library function because
//! `cratestack-parser` and `cratestack-macros` don't share an IR-walking
//! crate today, and this fixed-point loop is small enough that
//! bug-for-bug parity is easy to keep by inspection (both operate on
//! the identical `cratestack_core::Schema` shape).

use std::collections::BTreeSet;

use cratestack_core::{Schema, TypeArity, TypeRef};

use crate::shared::{ident, is_computed_field, to_snake_case};

/// Names (of `type` declarations and `model`s) whose wire shape contains
/// at least one `@computed` field, directly or through nested `type`
/// fields — including fields typed as a `model` (a `type` field CAN
/// reference a `model` directly as a value, not just as a relation; see
/// `crate::types::generate_type_struct`'s `custom_in_super` doc). Model
/// relation fields never propagate: `wire_model_fields` (what a model's
/// own compose helper walks) excludes them entirely, matching how the
/// model response projection already treats relations as a separate,
/// explicitly-included concern.
pub(crate) fn computed_bearing_names(schema: &Schema) -> BTreeSet<String> {
    let mut bearing: BTreeSet<String> = schema
        .models
        .iter()
        .filter(|model| model.fields.iter().any(is_computed_field))
        .map(|model| model.name.clone())
        .chain(
            schema
                .types
                .iter()
                .filter(|ty| ty.fields.iter().any(is_computed_field))
                .map(|ty| ty.name.clone()),
        )
        .collect();

    loop {
        let mut grew = false;
        for ty in &schema.types {
            if bearing.contains(&ty.name) {
                continue;
            }
            if ty
                .fields
                .iter()
                .any(|field| bearing.contains(&field.ty.name))
            {
                bearing.insert(ty.name.clone());
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    bearing
}

/// `compose_<owner_snake>_value` — the single naming rule shared by the
/// helper's own definition ([`super::compose`]) and every call site that
/// invokes it (procedure dispatch, and other compose helpers recursing
/// into a nested computed-bearing owner).
pub(crate) fn compose_fn_ident(owner_name: &str) -> syn::Ident {
    ident(&format!("compose_{}_value", to_snake_case(owner_name)))
}

/// Mirrors `crate::procedure::types::is_list_arg`'s predicate (kept
/// private there — duplicated here rather than exported across an
/// unrelated module boundary for one boolean): a bare `T[]` return,
/// never `Page<T>`/`FindMany<T>` reusing the `List`-arity slot for their
/// own generic-argument shape.
fn is_bare_list(ty: &TypeRef) -> bool {
    matches!(ty.arity, TypeArity::List) && !ty.is_page() && !ty.is_find_many()
}

/// What (if anything) a procedure's return type needs composed before
/// encoding. `None` means the return type — after unwrapping `Page<T>`/
/// list arity — never reaches a computed-bearing owner, so dispatch
/// codegen must emit exactly what it did before this feature existed
/// (`docs/design/computed-fields.md`'s "Procedure outputs" section).
pub(crate) enum ProcedureOutputComposition {
    /// `T` or `T?` with `owner` computed-bearing. `optional` distinguishes
    /// `Option<T>` (needs a `None` -> `ProjectedValue::Null` arm) from a
    /// required `T`.
    Unary { owner: String, optional: bool },
    /// Bare `T[]` (never `Page<T>`/`FindMany<T>` — see [`is_bare_list`]).
    List { owner: String },
    /// `Page<T>` — the envelope (`items`/`totalCount`/`pageInfo`) is kept
    /// exactly as `cratestack_core::Page<T>`'s own `Serialize` impl
    /// shapes it; only each item is composed.
    Page { owner: String },
}

pub(crate) fn procedure_output_composition(
    return_type: &TypeRef,
    bearing: &BTreeSet<String>,
) -> Option<ProcedureOutputComposition> {
    if let Some(item) = return_type.page_item() {
        return bearing
            .contains(&item.name)
            .then(|| ProcedureOutputComposition::Page {
                owner: item.name.clone(),
            });
    }
    if is_bare_list(return_type) {
        return bearing
            .contains(&return_type.name)
            .then(|| ProcedureOutputComposition::List {
                owner: return_type.name.clone(),
            });
    }
    bearing
        .contains(&return_type.name)
        .then(|| ProcedureOutputComposition::Unary {
            owner: return_type.name.clone(),
            optional: matches!(return_type.arity, TypeArity::Optional),
        })
}

#[cfg(test)]
mod tests;
