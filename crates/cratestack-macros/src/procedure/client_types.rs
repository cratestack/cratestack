//! The client-side `Args` struct for a procedure.
//!
//! Same field set as the server's ([`super::types`]) — they share one
//! builder spec so they cannot drift — minus the `ProcedureArgs` impl,
//! which exists to feed policy evaluation and has nothing to evaluate on
//! a client that enforces no policy.
//!
//! Split from `types.rs` for the workspace's 200-line ceiling.

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeDecl};
use quote::quote;

use crate::builder::generate_builder;
use crate::shared::{bytes_serde_attr, doc_attrs, ident};

use super::type_tokens::procedure_type_tokens;
use super::types::procedure_arg_builder_fields;

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
