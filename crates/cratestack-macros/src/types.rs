//! Generators for the `type ...` and `enum ...` schema declarations,
//! plus the custom-field-resolver descriptors used at runtime.

mod enums;

use std::collections::BTreeSet;

use cratestack_core::TypeDecl;
use quote::quote;

use crate::shared::{
    doc_attrs, field_definition, ident, is_custom_field, rust_type_tokens_with_scope, schema_lit,
    to_snake_case, value_tokens,
};

pub(crate) use enums::{generate_client_enum_type, generate_enum_type};

pub(crate) fn generate_type_struct(
    ty: &TypeDecl,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    // `custom_in_super = true`: a `type` block's fields can reference not
    // just sibling types/enums (also declared in this `types` module) but
    // also a `model`, which lives in the sibling `models` module. `super::`
    // resolves either way — `pub use types::*` / `pub use models::*` at the
    // `cratestack_schema` level re-export both into scope from `super`.
    let fields = ty
        .fields
        .iter()
        .map(|field| field_definition(field, false, true));
    let arg_matches = ty.fields.iter().map(|field| {
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

pub(crate) fn generate_client_type_struct(ty: &TypeDecl) -> proc_macro2::TokenStream {
    let type_ident = ident(&ty.name);
    let docs = doc_attrs(&ty.docs);
    let fields = ty
        .fields
        .iter()
        .map(|field| field_definition(field, false, true));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #type_ident {
            #(#fields)*
        }
    }
}

pub(crate) fn generate_custom_field_descriptors(ty: &TypeDecl) -> Vec<proc_macro2::TokenStream> {
    ty.fields
        .iter()
        .filter(|field| is_custom_field(field))
        .map(|field| {
            let owner = schema_lit(&ty.name);
            let field_name = schema_lit(&field.name);
            let resolver_method = schema_lit(&format!(
                "resolve_{}_{}",
                to_snake_case(&ty.name),
                to_snake_case(&field.name)
            ));
            quote! {
                CustomFieldDescriptor {
                    owner: #owner,
                    field: #field_name,
                    resolver_method: #resolver_method,
                }
            }
        })
        .collect()
}

pub(crate) fn generate_custom_field_resolver_methods(
    ty: &TypeDecl,
) -> Vec<proc_macro2::TokenStream> {
    let type_ident = ident(&ty.name);
    ty.fields
        .iter()
        .filter(|field| is_custom_field(field))
        .map(|field| {
            let method_ident = ident(&format!(
                "resolve_{}_{}",
                to_snake_case(&ty.name),
                to_snake_case(&field.name)
            ));
            let return_type = rust_type_tokens_with_scope(&field.ty, true);

            quote! {
                fn #method_ident(
                    &self,
                    source: &super::#type_ident,
                    ctx: &::cratestack::CoolContext,
                ) -> impl ::core::future::Future<Output = Result<#return_type, ::cratestack::CoolError>> + Send;
            }
        })
        .collect()
}
