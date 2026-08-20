//! Turning schema [`Field`]s into [`BuilderField`] specs.
//!
//! Two constructors, one per type-token strategy already in the codebase:
//! [`model_builder_fields`] mirrors
//! [`crate::model::struct_only::struct_field_definition`] (models, CRUD
//! inputs, views — enum-typed fields resolve through `super::types::`),
//! and [`scoped_builder_fields`] mirrors
//! [`crate::shared::field_definition`] (`type` blocks, which sit *in* the
//! `types` module and so name siblings directly). Both call the very same
//! type-token function their struct-definition counterpart calls, so a
//! setter's argument type can never drift from the field it fills.

use std::collections::BTreeSet;

use cratestack_core::{Field, TypeArity};

use crate::model::struct_only::struct_field_type;
use crate::shared::{doc_attrs, field_type, ident};

use super::BuilderField;

/// The element type of a list-arity field's append setter, `None` for
/// every other field. Computed by calling the *same* `field_ty` function
/// that already typed [`BuilderField::ty`], on a clone of the field with
/// its arity forced to `Required` — arity is the only input that turns a
/// scalar type into `Vec<T>` in either type-token function, so this can
/// never drift from the field's own list type the way a hand-rolled
/// "strip the `Vec<>`" step could.
fn list_elem_ty(
    field: &Field,
    field_ty: impl Fn(&Field, bool) -> proc_macro2::TokenStream,
) -> Option<proc_macro2::TokenStream> {
    if !matches!(field.ty.arity, TypeArity::List) {
        return None;
    }
    let mut scalar = field.clone();
    scalar.ty.arity = TypeArity::Required;
    Some(field_ty(&scalar, false))
}

/// A field must be filled unless its emitted type is `Option<T>` or
/// `Vec<T>` — see [`super`]'s "what counts as optional".
///
/// `Page<T>` is the one shape where arity lies: `rust_type_tokens_with_scope`
/// returns `Page<T>` for it regardless of the declared arity, so an
/// `Optional` `Page<T>` would still be a non-`Option` field.
fn is_required(field: &Field, wrap_for_patch: bool) -> bool {
    if wrap_for_patch {
        // Every update-input field is wrapped to `Option<..>` — "the
        // caller didn't touch this" is precisely the default.
        return false;
    }
    field.ty.is_page() || matches!(field.ty.arity, TypeArity::Required)
}

/// `impl Into<T>` setters, restricted to plain required `String`-backed
/// fields. `&str` / `String` / `Cow<str>` are the only meaningful sources,
/// so inference stays unambiguous — unlike the numeric types, where an
/// extra inference variable makes `.count(1)` depend on integer-literal
/// fallback rather than on the field's declared width.
fn takes_into(field: &Field) -> bool {
    matches!(field.ty.arity, TypeArity::Required)
        && matches!(field.ty.name.as_str(), "String" | "Cuid")
}

/// Same rule as [`takes_into`], applied to a list field's *element* type
/// instead of the field's own type — `.add_tags("rust")` should work
/// without an explicit `.to_owned()` exactly when `.tags(vec!["rust".into()])`
/// would.
fn takes_into_elem(field: &Field) -> bool {
    matches!(field.ty.arity, TypeArity::List) && matches!(field.ty.name.as_str(), "String" | "Cuid")
}

/// Shared assembly: `field_ty(field, wrap_for_patch)` types the struct
/// field, and on a patch struct `field_ty(field, false)` types the setter
/// — the same type the un-patched struct would have carried.
fn build_spec(
    field: &Field,
    wrap_for_patch: bool,
    field_ty: impl Fn(&Field, bool) -> proc_macro2::TokenStream,
) -> BuilderField {
    let spec = BuilderField::new(
        ident(&field.name),
        field_ty(field, wrap_for_patch),
        is_required(field, wrap_for_patch),
    );
    let spec = if wrap_for_patch {
        spec.with_patch(field_ty(field, false))
    } else {
        spec
    };
    let spec = spec.with_into(takes_into(field));
    let spec = match list_elem_ty(field, &field_ty) {
        Some(elem_ty) => {
            let append_ident = ident(&format!("add_{}", field.name));
            spec.with_list(elem_ty, takes_into_elem(field), append_ident)
        }
        None => spec,
    };
    spec.with_docs(doc_attrs(&field.docs))
}

pub(crate) fn model_builder_fields<'a>(
    fields: impl IntoIterator<Item = &'a Field>,
    wrap_for_patch: bool,
    enum_names: &BTreeSet<&str>,
) -> Vec<BuilderField> {
    fields
        .into_iter()
        .map(|field| {
            build_spec(field, wrap_for_patch, |field, patch| {
                struct_field_type(field, patch, enum_names)
            })
        })
        .collect()
}

pub(crate) fn scoped_builder_fields<'a>(
    fields: impl IntoIterator<Item = &'a Field>,
    wrap_for_patch: bool,
    custom_in_super: bool,
) -> Vec<BuilderField> {
    fields
        .into_iter()
        .map(|field| {
            build_spec(field, wrap_for_patch, |field, patch| {
                field_type(field, patch, custom_in_super)
            })
        })
        .collect()
}
