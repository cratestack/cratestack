//! Small ident / doc / set helpers shared across every macro
//! generator. Anything bigger lives in sibling submodules.

mod attrs;
pub(crate) mod decimal_backend;
mod procedure_attrs;
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
pub(crate) use procedure_attrs::is_stream_procedure;
pub(crate) use sql::{create_sql_value, sql_value_tokens, update_sql_value};
pub(crate) use types::{
    field_definition, field_type, query_scalar_list_parser_tokens, query_scalar_parser_tokens,
    rust_type_tokens, rust_type_tokens_with_scope,
};
pub(crate) use value::value_tokens;

pub(crate) fn schema_lit(value: &str) -> LitStr {
    LitStr::new(value, proc_macro2::Span::call_site())
}

/// Turns a schema-authored name (field, relation, etc.) into a Rust
/// identifier, escaping it as a raw identifier (`r#type`) when it collides
/// with a Rust keyword — see cratestack#398. `self`/`Self`/`super`/`crate`
/// have no valid identifier spelling at all (not even raw); those are
/// rejected earlier, at schema-parse time
/// (`cratestack_parser::validate::fields`), so by the time codegen calls
/// this function they should never appear here. If one slips through
/// anyway, fall back to a plain (non-raw) identifier rather than panicking
/// — `syn::Ident::new_raw` panics on exactly those four strings — so the
/// failure surfaces as an ordinary `rustc` parse error instead of a macro
/// panic.
pub(crate) fn ident(value: &str) -> syn::Ident {
    if cratestack_core::rust_keywords::is_raw_escapable_keyword(value) {
        syn::Ident::new_raw(value, proc_macro2::Span::call_site())
    } else {
        syn::Ident::new(value, proc_macro2::Span::call_site())
    }
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

// cratestack#345: this is the server's real, load-bearing REST route
// algorithm (see `axum::model::routes::generate_model_axum_routes`).
// Sourced from `cratestack_core::route_naming` — already a shared
// dependency of this crate and of `cratestack-client-typescript` /
// `cratestack-client-dart` — so the client generators import the exact
// same implementation instead of reimplementing it. Do not redefine these
// locally.
pub(crate) use cratestack_core::route_naming::{pluralize, to_snake_case};
