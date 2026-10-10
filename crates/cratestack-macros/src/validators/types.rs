//! `ValidateFields` impls for `type` and `model` declarations and procedure
//! `Args`: the validators a field declares, enforced on the values a client
//! sends (ADR 0019 D5, PR A).
//!
//! Before this, `@length` on a `type` field was accepted and validated
//! nothing: only a model's create and update inputs ran validators. A
//! procedure argument is a `type` or a `model` (or a list or optional of
//! one), and so is a `@computed` params type, so their validators run here,
//! through the same field emitter model inputs use.
//!
//! Only what a client sends is validated. A `type` that a procedure only
//! returns is built by the server, and validating the server's own output
//! would turn a server bug into a client-visible failure after the work is
//! done; `cratestack check` refuses a validator on such a `type`
//! (`cratestack-parser/src/validate/type_validator_reach.rs`), so none is
//! silently inert. The call sites are the generated procedure helpers, which
//! see arguments and nothing else (`crate::procedure::instrument`), and the
//! `?computedParams=` parser (`crate::axum::model::computed`).
//!
//! The field path is a chain of borrowed segments (`FieldPath`) rendered
//! only when a validator fails, so a valid request allocates nothing for it.

use std::collections::BTreeSet;

use cratestack_core::{Model, Procedure, TypeArity, TypeDecl, TypeRef};
use proc_macro2::TokenStream;
use quote::quote;

use crate::shared::ident;

use super::emit::emit_field_validators;
use super::validating::{stored_model_fields, stored_type_fields};
use super::{FieldScope, Validating, parse_field_validators};

/// The call that validates `value`, a field `name` of the value at `path`,
/// when its `type` or `model` needs one.
fn nested(name: &str, ty: &TypeRef, value: TokenStream, validating: &Validating) -> TokenStream {
    if !validating.contains(&ty.name) {
        return quote! {};
    }
    match ty.arity {
        TypeArity::Required => quote! {
            #value.validate_at(&path.field(#name))?;
        },
        TypeArity::Optional => quote! {
            if let Some(inner) = #value.as_ref() {
                inner.validate_at(&path.field(#name))?;
            }
        },
        TypeArity::List => quote! {
            {
                let list_path = path.field(#name);
                for (index, item) in #value.iter().enumerate() {
                    item.validate_at(&list_path.index(index))?;
                }
            }
        },
    }
}

fn validate_impl(target: TokenStream, body: TokenStream) -> TokenStream {
    quote! {
        impl ::cratestack::ValidateFields for #target {
            fn validate_at(
                &self,
                path: &::cratestack::FieldPath<'_>,
            ) -> ::std::result::Result<(), ::cratestack::CratestackError> {
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
pub(crate) fn generate_type_validate_impl(ty: &TypeDecl, validating: &Validating) -> TokenStream {
    if !validating.contains(&ty.name) {
        return quote! {};
    }
    let fields = stored_type_fields(ty).map(|field| {
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

/// `impl ValidateFields for <model>`, or nothing when no stored field of
/// the model carries a validator. This is the model sent as a value, not the
/// model's create or update input (`generate_input_validate_body` runs
/// those): a procedure argument of the model's type, or a field of a `type`
/// holding one, was decoded with no validator run on it.
pub(crate) fn generate_model_validate_impl(
    model: &Model,
    model_names: &BTreeSet<&str>,
    validating: &Validating,
) -> TokenStream {
    if !validating.contains(&model.name) {
        return quote! {};
    }
    let fields = stored_model_fields(model, model_names).filter_map(|field| {
        let own = parse_field_validators(field);
        (!own.is_empty()).then(|| emit_field_validators(field, &own, false, FieldScope::Nested))
    });
    let model_ident = ident(&model.name);
    validate_impl(quote! { #model_ident }, quote! { #(#fields)* })
}

/// Whether any argument of `procedure` is, or holds, a validated `type` or
/// `model`: `Args` then implements `ValidateFields` and the generated
/// helpers call it.
pub(crate) fn procedure_validates_args(procedure: &Procedure, validating: &Validating) -> bool {
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
    validating: &Validating,
) -> TokenStream {
    if !procedure_validates_args(procedure, validating) {
        return quote! {};
    }
    let checks = procedure.args.iter().map(|arg| {
        let arg_ident = ident(&arg.name);
        nested(&arg.name, &arg.ty, quote! { self.#arg_ident }, validating)
    });
    validate_impl(quote! { Args }, quote! { #(#checks)* })
}
