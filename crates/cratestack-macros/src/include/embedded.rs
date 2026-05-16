//! `include_embedded_schema!` composer — emits the embedded ORM
//! surface backed by rusqlite. No sqlx, no axum, no procedures.

use std::collections::BTreeSet;

use proc_macro::TokenStream;
use quote::quote;
use syn::LitStr;

use crate::model::{
    generate_create_input_struct, generate_field_module, generate_model_descriptor,
    generate_model_struct_only, generate_primary_key_accessor_impl,
    generate_rusqlite_from_row_impl, generate_update_input_struct, generate_upsert_input_struct,
};
use crate::shared::schema_lit;
use crate::types::{generate_enum_type, generate_type_struct};

use super::parse::parse_schema_literal;

pub(super) fn compose_embedded_schema(schema_path: &LitStr) -> TokenStream {
    let (schema_relative, resolved, schema) = match parse_schema_literal(schema_path) {
        Ok(parsed) => parsed,
        Err(error) => return error,
    };
    let resolved_literal = resolved.display().to_string();

    let mixin_names = schema.mixins.iter().map(|mixin| schema_lit(&mixin.name));
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
    let type_structs = schema
        .types
        .iter()
        .map(|ty| generate_type_struct(ty, &enum_name_set));
    let enum_types = schema.enums.iter().map(generate_enum_type);
    let model_structs = schema
        .models
        .iter()
        .map(|model| generate_model_struct_only(model, &model_name_set, &enum_name_set));
    let rusqlite_from_row_impls = schema
        .models
        .iter()
        .map(|model| generate_rusqlite_from_row_impl(model, &model_name_set, &enum_name_set));
    let primary_key_accessor_impls = schema
        .models
        .iter()
        .map(generate_primary_key_accessor_impl)
        .collect::<Vec<_>>();
    let auth = schema.auth.as_ref();
    let model_descriptors = match schema
        .models
        .iter()
        .map(|model| generate_model_descriptor(model, &schema.models, &schema.types, auth))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(descriptors) => descriptors,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let field_modules = match schema
        .models
        .iter()
        .map(|model| generate_field_module(model, &model_name_set, &schema.models))
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(field_modules) => field_modules,
        Err(error) => {
            return syn::Error::new(schema_path.span(), error)
                .to_compile_error()
                .into();
        }
    };
    let create_input_structs = schema
        .models
        .iter()
        .map(|model| generate_create_input_struct(model, &model_name_set, &enum_name_set));
    let update_input_structs = schema
        .models
        .iter()
        .map(|model| generate_update_input_struct(model, &model_name_set, &enum_name_set));
    let upsert_input_impls = schema
        .models
        .iter()
        .map(|model| generate_upsert_input_struct(model, &model_name_set, &enum_name_set))
        .collect::<Vec<_>>();

    // Procedures are skipped on the embedded path — local apps don't have an
    // RPC surface to call. `@@audit` and `@@emit` directives are silently
    // ignored for v1; see CHANGELOG for the follow-up plan.

    let expanded = quote! {
        pub mod cratestack_schema {
            pub const SCHEMA_PATH: &str = #schema_relative;
            pub const SCHEMA_SOURCE: &str = include_str!(#resolved_literal);
            pub const MIXINS: &[&str] = &[#(#mixin_names),*];
            pub const MODELS: &[&str] = &[#(#model_names),*];
            pub const TYPES: &[&str] = &[#(#type_names),*];
            pub const ENUMS: &[&str] = &[#(#enum_names),*];

            pub const MIXIN_COUNT: usize = MIXINS.len();
            pub const MODEL_COUNT: usize = MODELS.len();
            pub const TYPE_COUNT: usize = TYPES.len();
            pub const ENUM_COUNT: usize = ENUMS.len();

            pub mod types {
                #(#enum_types)*
                #(#type_structs)*
            }

            pub use types::*;

            pub mod models {
                #(#model_structs)*
                #(#rusqlite_from_row_impls)*
                #(#primary_key_accessor_impls)*
                #(#model_descriptors)*
            }

            pub use models::*;

            #(#field_modules)*

            pub mod inputs {
                #(#create_input_structs)*
                #(#update_input_structs)*
                #(#upsert_input_impls)*
            }

            pub use inputs::*;
        }
    };

    expanded.into()
}
