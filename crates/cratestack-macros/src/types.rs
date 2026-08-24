//! Generators for the `type ...` and `enum ...` schema declarations.
//! `@computed`-field descriptor/resolver generation lives in
//! `crate::computed` — it spans both `type` and `model` declarations, so
//! it doesn't fit cleanly under this module's `type`-only scope.

mod enums;

use std::collections::BTreeSet;

use cratestack_core::TypeDecl;
use quote::quote;

use crate::builder::{
    generate_builder, scoped_builder_fields, scoped_builder_fields_with_wire_scope,
};
use crate::shared::{
    doc_attrs, field_definition, field_definition_with_wire_scope, ident, is_computed_field,
    value_tokens,
};

pub(crate) use enums::{generate_client_enum_type, generate_enum_type};

/// Server-side `type` struct — EXCLUDES `@computed` fields (never stored
/// or hand-constructed server-side; see `docs/design/computed-fields.md`).
/// The client-side counterpart, [`generate_client_type_struct`], includes
/// them — that's the wire response shape.
pub(crate) fn generate_type_struct(
    ty: &TypeDecl,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    let stored_fields: Vec<_> = ty
        .fields
        .iter()
        .filter(|field| !is_computed_field(field))
        .collect();
    // `custom_in_super = true`: a `type` block's fields can reference not
    // just sibling types/enums (also declared in this `types` module) but
    // also a `model`, which lives in the sibling `models` module. `super::`
    // resolves either way — `pub use types::*` / `pub use models::*` at the
    // `cratestack_schema` level re-export both into scope from `super`.
    let fields = stored_fields
        .iter()
        .map(|field| field_definition(field, false, true));
    let builder = generate_builder(
        &type_ident,
        &scoped_builder_fields(stored_fields.iter().copied(), false, true),
    );
    let arg_matches = stored_fields.iter().map(|field| {
        let field_name = &field.name;
        let field_ident = ident(&field.name);
        let value = value_tokens(quote! { self.#field_ident.clone() }, &field.ty, enum_names);
        quote! {
            #field_name => Some(#value),
        }
    });

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #type_ident {
            #(#fields)*
        }

        #builder

        impl ::cratestack::ProcedureArgs for #type_ident {
            fn procedure_arg_value(&self, field: &str) -> Option<::cratestack::Value> {
                match field {
                    #(#arg_matches)*
                    _ => None,
                }
            }
        }
    }
}

/// Client-side `type` struct — INCLUDES `@computed` fields (the wire
/// response shape; see `docs/design/computed-fields.md`).
pub(crate) fn generate_client_type_struct(ty: &TypeDecl) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    let fields = ty
        .fields
        .iter()
        .map(|field| field_definition(field, false, true));
    let builder = generate_builder(&type_ident, &scoped_builder_fields(&ty.fields, false, true));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #type_ident {
            #(#fields)*
        }

        #builder
    }
}

/// [`generate_client_type_struct`]'s wire-scope counterpart, for the
/// server composer's dedicated `wire` module (`crate::computed::wire`) —
/// emitted only for a computed-bearing `type`. Field-for-field identical
/// except a field naming another computed-bearing owner resolves to the
/// sibling `super::wire::<Owner>` instead of the plain server-side
/// `super::<Owner>` — see `crate::shared::rust_type_tokens_with_wire_scope`'s
/// doc. `bearing` is the schema-wide computed-bearing set, so this handles
/// both "a `type` field nests another bearing `type`" (`Card.cover`) and
/// "a `type` field references a bearing `model` directly" cases.
pub(crate) fn generate_wire_type_struct(
    ty: &TypeDecl,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    let fields = ty
        .fields
        .iter()
        .map(|field| field_definition_with_wire_scope(field, bearing));
    let builder = generate_builder(
        &type_ident,
        &scoped_builder_fields_with_wire_scope(&ty.fields, bearing),
    );

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #type_ident {
            #(#fields)*
        }

        #builder
    }
}
