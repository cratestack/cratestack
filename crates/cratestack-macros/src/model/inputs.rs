//! CRUD input structs (`Create{Model}Input`, `Update{Model}Input`)
//! plus the upsert impl for client-supplied PKs. Server variants get
//! `sql_values()`/`validate()` impls; client variants are bare structs.

use std::collections::BTreeSet;

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{
    create_sql_value, generated_doc_attr, ident, is_generated_on_create, is_primary_key,
    is_readonly_field, is_server_only_field, is_version_field, scalar_model_fields,
    sql_value_tokens, update_sql_value,
};
use crate::validators::generate_input_validate_body;

use super::struct_only::struct_field_definition;

fn create_input_fields<'a>(model: &'a Model, model_names: &BTreeSet<&str>) -> Vec<&'a Field> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|f| !is_generated_on_create(f) && !is_readonly_field(f))
        .filter(|f| !is_server_only_field(f) && !is_version_field(f))
        .collect()
}

fn update_input_fields<'a>(model: &'a Model, model_names: &BTreeSet<&str>) -> Vec<&'a Field> {
    scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|f| !is_primary_key(f) && !is_readonly_field(f))
        .filter(|f| !is_server_only_field(f) && !is_version_field(f))
        .collect()
}

fn validate_impl_tokens(fields: &[&Field], partial: bool) -> proc_macro2::TokenStream {
    let Some(body) = generate_input_validate_body(fields, partial) else {
        return quote! {};
    };
    quote! {
        fn validate(&self) -> ::std::result::Result<(), ::cratestack::CoolError> {
            #body
        }
    }
}

pub(crate) fn generate_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields = create_input_fields(model, model_names);
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| create_sql_value(field, enum_names));
    let model_ident = ident(&model.name);
    let validate_impl = validate_impl_tokens(&fields, false);

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::CreateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                vec![#(#sql_values),*]
            }
            #validate_impl
        }
    }
}

pub(crate) fn generate_client_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields = create_input_fields(model, model_names);
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}

pub(crate) fn generate_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields = update_input_fields(model, model_names);
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| update_sql_value(field, enum_names));
    let model_ident = ident(&model.name);
    let validate_impl = validate_impl_tokens(&fields, true);

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::UpdateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                let mut values = Vec::new();
                #(#sql_values)*
                values
            }
            #validate_impl
        }
    }
}

/// Emit `impl UpsertModelInput<M> for Create{Model}Input` for models
/// whose primary key is client-supplied. Server-generated PKs get no
/// upsert impl — calling `.upsert(...)` is a compile error.
pub(crate) fn generate_upsert_input_struct(
    model: &Model,
    _model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    // Server-generated PK → no upsert impl; v1 doesn't support
    // unique-key conflict targets yet.
    let Some(primary_key) = model.fields.iter().find(|f| is_primary_key(f)) else {
        return quote! {};
    };
    if is_generated_on_create(primary_key) {
        return quote! {};
    }

    let input_ident = ident(&format!("Create{}Input", model.name));
    let model_ident = ident(&model.name);
    let pk_field_ident = ident(&primary_key.name);
    let pk_value =
        sql_value_tokens(quote! { self.#pk_field_ident.clone() }, &primary_key.ty, enum_names);

    // sql_values()/validate() defer to CreateModelInput on the same
    // struct (keeps validators in one place); fully qualified to
    // disambiguate when both traits are in scope.
    quote! {
        impl ::cratestack::UpsertModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                <Self as ::cratestack::CreateModelInput<super::models::#model_ident>>::sql_values(self)
            }

            fn primary_key_value(&self) -> ::cratestack::SqlValue {
                #pk_value
            }

            fn validate(&self) -> ::std::result::Result<(), ::cratestack::CoolError> {
                <Self as ::cratestack::CreateModelInput<super::models::#model_ident>>::validate(self)
            }
        }
    }
}

pub(crate) fn generate_client_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields = update_input_fields(model, model_names);
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}
