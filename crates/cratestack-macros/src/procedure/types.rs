//! Argument struct + return-type token generation shared between
//! the server `pub mod <procedure>` (for `authorize` / `invoke`) and
//! the lighter client-side module.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeArity, TypeDecl, TypeRef};
use quote::quote;

use crate::shared::{doc_attrs, ident, value_tokens};

pub(super) fn generate_procedure_args_struct(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let args_ident = ident("Args");
    let definitions = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_type = procedure_type_tokens(&arg.ty, types, enum_names);
        let docs = doc_attrs(&arg.docs);
        quote! {
            #docs
            pub #field_ident: #field_type,
        }
    });
    let value_matches = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_name = &arg.name;
        let value = value_tokens(quote! { self.#field_ident.clone() }, &arg.ty, enum_names);
        quote! { #field_name => Some(#value), }
    });
    let nested_arg_match = procedure
        .args
        .iter()
        .find(|arg| arg.name == "args")
        .and_then(|arg| types.iter().find(|candidate| candidate.name == arg.ty.name))
        .map(|_| {
            quote! {
                _ if field.starts_with("args.") => self.args.procedure_arg_value(&field[5..]),
                _ => self.args.procedure_arg_value(field),
            }
        })
        .unwrap_or_else(|| {
            quote! {
                _ => None,
            }
        });

    let default_derive = if procedure.args.is_empty() {
        quote! { , Default }
    } else {
        quote! {}
    };

    quote! {
        #[doc = "Generated argument payload for this procedure."]
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize #default_derive)]
        pub struct #args_ident {
            #(#definitions)*
        }

        impl ::cratestack::ProcedureArgs for #args_ident {
            fn procedure_arg_value(&self, field: &str) -> Option<::cratestack::Value> {
                match field {
                    #(#value_matches)*
                    #nested_arg_match
                }
            }
        }
    }
}

pub(super) fn generate_client_procedure_args_struct(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let args_ident = ident("Args");
    let definitions = procedure.args.iter().map(|arg| {
        let field_ident = ident(&arg.name);
        let field_type = procedure_type_tokens(&arg.ty, types, enum_names);
        let docs = doc_attrs(&arg.docs);
        quote! {
            #docs
            pub #field_ident: #field_type,
        }
    });

    let default_derive = if procedure.args.is_empty() {
        quote! { , Default }
    } else {
        quote! {}
    };

    quote! {
        #[doc = "Generated argument payload for this procedure."]
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize #default_derive)]
        pub struct #args_ident {
            #(#definitions)*
        }
    }
}

pub(super) fn procedure_output_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    procedure_type_tokens(type_ref, types, enum_names)
}

pub(crate) fn procedure_client_output_item_tokens(type_ref: &TypeRef) -> proc_macro2::TokenStream {
    match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => quote! { ::cratestack::Decimal },
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let model_ident = ident(other);
            quote! { super::#model_ident }
        }
    }
}

fn procedure_type_tokens(
    type_ref: &TypeRef,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    if type_ref.is_page() {
        let item = type_ref
            .page_item()
            .expect("validated Page<T> should include an item type");
        let item_type = procedure_type_tokens(item, types, enum_names);
        return quote! { ::cratestack::Page<#item_type> };
    }

    let inner = match type_ref.name.as_str() {
        "String" => quote! { String },
        "Cuid" => quote! { String },
        "Int" => quote! { i64 },
        "Float" => quote! { f64 },
        "Boolean" => quote! { bool },
        "DateTime" => quote! { ::cratestack::chrono::DateTime<::cratestack::chrono::Utc> },
        "Decimal" => quote! { ::cratestack::Decimal },
        "Json" => quote! { ::cratestack::Json<::cratestack::Value> },
        "Bytes" => quote! { Vec<u8> },
        "Uuid" => quote! { ::cratestack::uuid::Uuid },
        other => {
            let item_ident = ident(other);
            if types.iter().any(|ty| ty.name == other) || enum_names.contains(other) {
                quote! { super::super::types::#item_ident }
            } else {
                quote! { super::super::#item_ident }
            }
        }
    };

    match type_ref.arity {
        TypeArity::Required => inner,
        TypeArity::Optional => quote! { Option<#inner> },
        TypeArity::List => quote! { Vec<#inner> },
    }
}
