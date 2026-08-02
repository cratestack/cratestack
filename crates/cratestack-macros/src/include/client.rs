//! `include_client_schema!` composer — emits the HTTP client surface:
//! model/input/procedure stubs for talking to a server over the wire.
//! No DB at all. For a `transport grpc` schema (ticket #209), also emits a
//! native `tonic`-based gRPC client under `cratestack_schema::grpc` — see
//! [`grpc`]'s module doc.

mod grpc;

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::client::generate_client_module;
use crate::model::{
    generate_client_create_input_struct, generate_client_field_module,
    generate_client_model_struct, generate_client_update_input_struct,
};
use crate::procedure::generate_client_procedure_module;
use crate::shared::schema_lit;
use crate::types::{generate_client_enum_type, generate_client_type_struct};

use super::parse::parse_schema_literal;

pub(super) fn compose_client_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema, schema_sha256) = match parse_schema_literal(schema_path)
    {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    if let Err(error) = super::reject_grpc::guard_client_grpc_transport(schema_path, &schema) {
        return error;
    }
    let grpc_module = match grpc::build_client_grpc_module(&schema, &resolved, schema_path) {
        Ok(tokens) => tokens,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let model_names = schema.models.iter().map(|model| schema_lit(&model.name));
    let model_name_set = schema
        .models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<BTreeSet<_>>();
    let type_names = schema.types.iter().map(|ty| schema_lit(&ty.name));
    let enum_names = schema
        .enums
        .iter()
        .map(|enum_decl| schema_lit(&enum_decl.name));
    let enum_name_set = crate::shared::enum_name_set(&schema.enums);
    let procedure_names = schema
        .procedures
        .iter()
        .map(|procedure| schema_lit(&procedure.name));
    let type_structs = schema.types.iter().map(generate_client_type_struct);
    let enum_types = schema.enums.iter().map(generate_client_enum_type);
    let model_structs = schema
        .models
        .iter()
        .map(|model| generate_client_model_struct(model, &model_name_set, &enum_name_set));
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_client_create_input_struct(model, &model_name_set, &enum_name_set));
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_client_update_input_struct(model, &model_name_set, &enum_name_set));
    // Client field modules: same surface as server field modules minus
    // emissions that hard-reference `*_MODEL` descriptors (which the
    // client composer doesn't emit). See `FieldModuleKind::Client`.
    let field_modules = match schema
        .models
        .iter()
        .map(|model| generate_client_field_module(model, &model_name_set, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(field_modules) => field_modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let procedure_modules = match schema
        .procedures
        .iter()
        .map(|procedure| generate_client_procedure_module(procedure, &schema.types, &enum_name_set))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(modules) => modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let generated_client_module =
        match generate_client_module(&schema.models, &schema.procedures, schema.transport) {
            Ok(module) => module,
            Err(error) => {
                return syn::Error::new(schema_path.span(), error)
                    .to_compile_error()
                    .into();
            }
        };

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            /// Hex-encoded SHA-256 of `SCHEMA_SOURCE`'s raw bytes, computed
            /// once at macro-expansion time. Not cryptographic-strength
            /// integrity — it's a drift-detection fingerprint: the generated
            /// `client::Client::new` below stamps it onto every
            /// `CratestackClient` it wraps, so it rides along as the
            /// `x-cratestack-schema-sha` header on every request; the
            /// server-side counterpart `tracing::warn!`s on mismatch, never
            /// rejects. See issue #178.
            pub const SCHEMA_SHA256: &str = #schema_sha256;
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];
            pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];

            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();
            pub const PROCEDURE_COUNT: usize = PROCEDURES.len();

            pub mod types {
                use ::cratestack::serde;

                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                use ::cratestack::serde;

                #(#model_structs)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                use ::cratestack::serde;

                #(#create_input_structs)*
                #(#update_input_structs)*
            }

            pub use inputs::*;

            #generated_client_module

            pub mod procedures {
                use ::cratestack::serde;

                #(#procedure_modules)*
            }

            #grpc_module
        }
    };

    expanded.into()
}
