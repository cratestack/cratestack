//! `ValidateFields` impls for `type` declarations and procedure `Args`: the
//! validators a `type` field declares, enforced on the arguments a client
//! sends (ADR 0019 D5, PR A).
//!
//! Before this, `@length` on a `type` field was accepted and validated
//! nothing: only a model's create and update inputs ran validators. A
//! procedure argument is a `type` (or a list or optional of one), so its
//! validators run here, through the same field emitter models use.
//!
//! Only arguments are validated. A `type` that a procedure only returns is
//! built by the server, and validating the server's own output would turn a
//! server bug into a client-visible failure after the work is done: the only
//! call site is the generated `authorize_with_db`, which sees arguments and
//! nothing else. A return-only `type` with a validator still gets its impl
//! (it is cheap, and no analysis of which `type`s are arguments is needed);
//! nothing calls it.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeArity, TypeDecl, TypeRef};
use proc_macro2::TokenStream;
use quote::quote;

use crate::shared::{ident, is_computed_field};

use super::emit::emit_field_validators;
use super::{FieldScope, parse_field_validators};

/// The fields the server-side struct of `ty` holds: a `@computed` field is
/// resolved on the way out and never decoded from a client.
fn stored_fields(ty: &TypeDecl) -> impl Iterator<Item = &cratestack_core::Field> {
    ty.fields.iter().filter(|field| !is_computed_field(field))
}

/// The `type`s whose values need validating: one with a validator on a
/// stored field, and any `type` with a stored field of such a `type`
/// (a fixpoint, so nesting of any depth and a cycle are both handled).
pub(crate) fn validating_type_names(types: &[TypeDecl]) -> BTreeSet<String> {
    let mut validating: BTreeSet<String> = types
        .iter()
        .filter(|ty| stored_fields(ty).any(|field| !parse_field_validators(field).is_empty()))
        .map(|ty| ty.name.clone())
        .collect();
    loop {
        let before = validating.len();
        for ty in types {
            if !validating.contains(&ty.name)
                && stored_fields(ty).any(|field| validating.contains(&field.ty.name))
            {
                validating.insert(ty.name.clone());
            }
        }
        if validating.len() == before {
            return validating;
        }
    }
}

/// The call that validates `value`, a field of `ty` named `name`, when its
/// `type` needs one; its path extends `path` by the field name, by
/// `[index]` for an element of a list.
fn nested(
    name: &str,
    ty: &TypeRef,
    value: TokenStream,
    validating: &BTreeSet<String>,
) -> TokenStream {
    if !validating.contains(&ty.name) {
        return quote! {};
    }
    match ty.arity {
        TypeArity::Required => quote! {
            #value.validate_at(&::std::format!("{}{}.", path, #name))?;
        },
        TypeArity::Optional => quote! {
            if let Some(inner) = #value.as_ref() {
                inner.validate_at(&::std::format!("{}{}.", path, #name))?;
            }
        },
        TypeArity::List => quote! {
            for (index, item) in #value.iter().enumerate() {
                item.validate_at(&::std::format!("{}{}[{}].", path, #name, index))?;
            }
        },
    }
}

fn validate_impl(target: TokenStream, body: TokenStream) -> TokenStream {
    quote! {
        impl ::cratestack::ValidateFields for #target {
            fn validate_at(&self, path: &str) -> ::std::result::Result<(), ::cratestack::CratestackError> {
                use ::cratestack::ValidateFields as _;
                let _ = path;
                #body
                Ok(())
            }
        }
    }
}

/// `impl ValidateFields for <ty>`, or nothing when `ty` has no validator
/// anywhere inside it.
pub(crate) fn generate_type_validate_impl(
    ty: &TypeDecl,
    validating: &BTreeSet<String>,
) -> TokenStream {
    if !validating.contains(&ty.name) {
        return quote! {};
    }
    let fields = stored_fields(ty).map(|field| {
        let own = parse_field_validators(field);
        let scalar = if own.is_empty() {
            quote! {}
        } else {
            emit_field_validators(field, &own, false, FieldScope::Nested)
        };
        let field_ident = ident(&field.name);
        let inner = nested(
            &field.name,
            &field.ty,
            quote! { self.#field_ident },
            validating,
        );
        quote! { #scalar #inner }
    });
    let type_ident = ident(&ty.name);
    validate_impl(quote! { #type_ident }, quote! { #(#fields)* })
}

/// Whether any argument of `procedure` is, or holds, a validated `type`:
/// `Args` then implements `ValidateFields` and `authorize_with_db` calls it.
pub(crate) fn procedure_validates_args(procedure: &Procedure, types: &[TypeDecl]) -> bool {
    let validating = validating_type_names(types);
    procedure
        .args
        .iter()
        .any(|arg| validating.contains(&arg.ty.name))
}

/// `impl ValidateFields for Args`, or nothing when no argument needs it.
/// Each argument is checked under its own name (`args.message`), which is
/// where the client wrote it.
pub(crate) fn generate_args_validate_impl(
    procedure: &Procedure,
    types: &[TypeDecl],
) -> TokenStream {
    let validating = validating_type_names(types);
    if !procedure
        .args
        .iter()
        .any(|arg| validating.contains(&arg.ty.name))
    {
        return quote! {};
    }
    let checks = procedure.args.iter().map(|arg| {
        let arg_ident = ident(&arg.name);
        nested(&arg.name, &arg.ty, quote! { self.#arg_ident }, &validating)
    });
    validate_impl(quote! { Args }, quote! { #(#checks)* })
}
