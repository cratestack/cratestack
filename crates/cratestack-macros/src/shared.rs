//! Small ident / doc / set helpers shared across every macro
//! generator. Anything bigger lives in sibling submodules.

mod attrs;
mod sql;
mod types;
mod value;

use std::collections::BTreeSet;

use cratestack_core::{EnumDecl, Field, Model};
use quote::quote;
use syn::LitStr;

pub(crate) use attrs::{
    auth_default_field, is_custom_field, is_generated_on_create, is_paged_model, is_pii_field,
    is_primary_key, is_readonly_field, is_sensitive_field, is_server_only_field, is_version_field,
    supports_comparison,
};
pub(crate) use sql::{create_sql_value, sql_value_tokens, update_sql_value};
pub(crate) use types::{
    field_definition, query_scalar_list_parser_tokens, query_scalar_parser_tokens,
    rust_type_tokens, rust_type_tokens_with_scope,
};
pub(crate) use value::value_tokens;

pub(crate) fn schema_lit(value: &str) -> LitStr {
    LitStr::new(value, proc_macro2::Span::call_site())
}

pub(crate) fn ident(value: &str) -> syn::Ident {
    syn::Ident::new(value, proc_macro2::Span::call_site())
}

pub(crate) fn doc_attrs(docs: &[String]) -> proc_macro2::TokenStream {
    let attrs = docs.iter().map(|doc| {
        quote! {
            #[doc = #doc]
        }
    });
    quote! {
        #(#attrs)*
    }
}

pub(crate) fn generated_doc_attr(doc: impl AsRef<str>) -> proc_macro2::TokenStream {
    let doc = doc.as_ref();
    quote! {
        #[doc = #doc]
    }
}

pub(crate) fn model_name_set(models: &[Model]) -> BTreeSet<&str> {
    models.iter().map(|model| model.name.as_str()).collect()
}

pub(crate) fn enum_name_set(enums: &[EnumDecl]) -> BTreeSet<&str> {
    enums
        .iter()
        .map(|enum_decl| enum_decl.name.as_str())
        .collect()
}

pub(crate) fn scalar_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| !is_relation_field(model_names, field))
        .collect()
}

pub(crate) fn relation_model_fields<'a>(
    model: &'a Model,
    model_names: &BTreeSet<&str>,
) -> Vec<&'a Field> {
    model
        .fields
        .iter()
        .filter(|field| is_relation_field(model_names, field))
        .collect()
}

pub(crate) fn is_relation_field(model_names: &BTreeSet<&str>, field: &Field) -> bool {
    model_names.contains(field.ty.name.as_str())
}

pub(crate) fn find_model<'a>(models: &'a [Model], name: &str) -> Option<&'a Model> {
    models.iter().find(|model| model.name == name)
}

pub(crate) fn to_snake_case(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if character.is_uppercase() {
            if index > 0 {
                output.push('_');
            }
            for lowercase in character.to_lowercase() {
                output.push(lowercase);
            }
        } else {
            output.push(character);
        }
    }
    output
}

pub(crate) fn pluralize(value: &str) -> String {
    if value.ends_with('s') {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}
