//! `include_server_schema!` composer — emits the full server surface:
//! sqlx Postgres backend, `Cratestack` runtime, axum router, procedure
//! handlers, events. No rusqlite anywhere in the output.

mod axum_dtos;
mod axum_module;
mod collect;
mod rpc_module;
mod runtime;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use super::parse::parse_schema_literal;

use collect::collect_server_schema;

pub(super) fn compose_server_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema) = match parse_schema_literal(schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let collected = match collect_server_schema(&schema, schema_path) {
        Ok(collected) => collected,
        Err(error) => return error,
    };

    let axum_module = axum_module::build_axum_module(&collected);
    let runtime_block =
        runtime::build_runtime_block(&collected.model_accessors, &collected.bound_model_accessors);

    // Destructure here for `quote!` interpolation — quoting through the
    // struct adds a `c.` prefix per field, which `quote!` doesn't accept.
    let collect::ServerCollected {
        transport_style_str,
        mixin_names,
        model_names,
        type_names,
        enum_names,
        procedure_names,
        type_structs,
        enum_types,
        custom_field_descriptors,
        custom_field_resolver_methods,
        model_structs,
        pg_from_row_impls,
        primary_key_accessor_impls,
        model_descriptors,
        field_modules,
        create_input_structs,
        update_input_structs,
        upsert_input_impls,
        procedure_modules,
        procedure_registry_methods,
        generated_client_module,
        generated_event_module,
        ..
    } = collected;

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            pub const MIXINS: &[&str] = &[#(#mixin_names),*];
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];
            pub const PROCEDURES: &[&str] = &[#(#procedure_names),*];

            pub const MIXIN_COUNT: usize = MIXINS.len();
            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();
            pub const PROCEDURE_COUNT: usize = PROCEDURES.len();

            /// Generation style the schema declared via the `transport`
            /// directive. Either `"rest"` (the default) or `"rpc"`. See
            /// `docs/design/rpc-transport.md`.
            pub const TRANSPORT_STYLE: &str = #transport_style_str;

            pub mod types {
                use ::cratestack::serde;

                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                use ::cratestack::serde;
                use ::cratestack::sqlx;

                #(#model_structs)*
                #(#pg_from_row_impls)*
                #(#primary_key_accessor_impls)*
                #(#model_descriptors)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                use ::cratestack::serde;

                #(#create_input_structs)*
                #(#update_input_structs)*
                #(#upsert_input_impls)*
            }

            pub use inputs::*;

            #generated_client_module
            #generated_event_module

            pub mod procedures {
                #(#procedure_modules)*

                pub trait ProcedureRegistry: Clone + Send + Sync + 'static {
                    #(#procedure_registry_methods)*
                }
            }

            pub mod custom {
                #[derive(Debug, Clone, Copy, PartialEq, Eq)]
                pub struct CustomFieldDescriptor {
                    pub owner: &'static str,
                    pub field: &'static str,
                    pub resolver_method: &'static str,
                }

                pub const FIELDS: &[CustomFieldDescriptor] = &[
                    #(#custom_field_descriptors),*
                ];

                pub const FIELD_COUNT: usize = FIELDS.len();

                pub trait CustomFieldResolver: Clone + Send + Sync + 'static {
                    #(#custom_field_resolver_methods)*
                }
            }

            pub use custom::CustomFieldResolver;

            pub const CUSTOM_FIELDS: &[custom::CustomFieldDescriptor] = custom::FIELDS;
            pub const CUSTOM_FIELD_COUNT: usize = custom::FIELD_COUNT;

            #axum_module

            #runtime_block
        }
    };

    expanded.into()
}
