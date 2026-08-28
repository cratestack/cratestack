//! Argument struct generation shared between the server `pub mod
//! <procedure>` (for `authorize` / `invoke`) and the lighter client-side
//! module. Return-type / element-type token resolution lives in the
//! sibling `type_tokens.rs` (split per the repo's 200-LoC file
//! convention) and is re-exported here for `procedure.rs`.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeArity, TypeDecl};
use quote::quote;

use crate::builder::{BuilderField, generate_builder};
use crate::shared::{bytes_serde_attr, doc_attrs, ident, value_tokens};

use super::type_tokens::procedure_type_tokens;

pub(crate) use super::type_tokens::procedure_client_output_item_tokens;
pub(super) use super::type_tokens::{procedure_output_tokens, procedure_stream_item_tokens};

/// `Args` field specs for a procedure's argument list, on both the server
/// and client sides — they emit the identical field set, so one builder
/// spec covers both `generate_procedure_args_struct` and
/// `generate_client_procedure_args_struct`.
///
/// Unlike [`crate::builder::model_builder_fields`], required-ness can't be
/// read off `arg.ty.arity` alone: [`procedure_type_tokens`] returns early
/// for `Page<T>`/`FindMany<T>` *before* applying arity, so those two
/// shapes are never `Option<_>`-typed regardless of what the schema
/// declared — matching that early return here keeps this in sync with the
/// type tokens actually emitted. The same early return means a `List`-arity
/// `Page<T>`/`FindMany<T>` arg (were a schema to declare one) is never
/// actually `Vec<_>`-typed either, so [`is_list_arg`] excludes both shapes
/// from getting an append setter for the identical reason.
fn is_list_arg(ty: &cratestack_core::TypeRef) -> bool {
    matches!(ty.arity, TypeArity::List) && !ty.is_page() && !ty.is_find_many()
}

fn procedure_arg_builder_fields(
    procedure: &Procedure,
    types: &[TypeDecl],
    enum_names: &BTreeSet<&str>,
) -> Vec<BuilderField> {
    procedure
        .args
        .iter()
        .map(|arg| {
            let field_ty = procedure_type_tokens(&arg.ty, types, enum_names);
            let required = arg.ty.is_page()
                || arg.ty.is_find_many()
                || matches!(arg.ty.arity, TypeArity::Required);
            let into = required && matches!(arg.ty.name.as_str(), "String" | "Cuid");
            let spec = BuilderField::new(ident(&arg.name), field_ty, required)
                .with_into(into)
                .with_docs(doc_attrs(&arg.docs));
            if is_list_arg(&arg.ty) {
                // Mirrors `builder/fields.rs::list_elem_ty` /
                // `takes_into_elem`: the element type is what this same
                // `arg.ty` would type as at `Required` arity, and `impl
                // Into<Elem>` follows the identical String/Cuid-only rule
                // as the scalar setters (`takes_into` above).
                let mut scalar = arg.ty.clone();
                scalar.arity = TypeArity::Required;
                let elem_ty = procedure_type_tokens(&scalar, types, enum_names);
                let elem_into = matches!(arg.ty.name.as_str(), "String" | "Cuid");
                let append_ident = ident(&format!("add_{}", arg.name));
                spec.with_list(elem_ty, elem_into, append_ident)
            } else {
                spec
            }
        })
        .collect()
}

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
        // An `Args` field carries no serde attributes of its own, so a
        // `Bytes` argument brings its whole `#[serde(...)]` list. This is
        // the case cratestack#783 was actually reported against — a
        // `Bytes` argument on an RPC procedure, where `POST
        // /rpc/procedure.<name>` decodes the body straight into this
        // struct.
        let serde_attr = bytes_serde_attr(&arg.ty, false);
        quote! {
            #docs
            #serde_attr
            pub #field_ident: #field_type,
        }
    });
    let builder = generate_builder(
        &args_ident,
        &procedure_arg_builder_fields(procedure, types, enum_names),
    );
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

        #builder

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
        // An `Args` field carries no serde attributes of its own, so a
        // `Bytes` argument brings its whole `#[serde(...)]` list. This is
        // the case cratestack#783 was actually reported against — a
        // `Bytes` argument on an RPC procedure, where `POST
        // /rpc/procedure.<name>` decodes the body straight into this
        // struct.
        let serde_attr = bytes_serde_attr(&arg.ty, false);
        quote! {
            #docs
            #serde_attr
            pub #field_ident: #field_type,
        }
    });
    let builder = generate_builder(
        &args_ident,
        &procedure_arg_builder_fields(procedure, types, enum_names),
    );

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

        #builder
    }
}
