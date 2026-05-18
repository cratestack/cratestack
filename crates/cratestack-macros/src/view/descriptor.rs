//! `ViewDescriptor` const emission.
//!
//! Mirrors [`crate::model::descriptor::generate_model_descriptor`] but
//! produces the narrower view-specific shape: no write policies, no
//! defaults/audit/version/PII state, no soft-delete column. Adds the
//! view-only fields `is_materialized` and `source_tables`.
//!
//! **v1 limitation**: this generator does not lower `@@allow("read", ...)`
//! predicates yet — the descriptor's policy arrays are emitted empty.
//! The parser-level validator (see
//! `crates/cratestack-parser/src/validate/views.rs`) still rejects any
//! non-`"read"` action on a view; runtime policy enforcement is
//! tracked as a follow-up.

use cratestack_core::View;
use quote::quote;

use crate::shared::{ident, to_snake_case};

pub(crate) fn generate_view_descriptor(view: &View) -> Result<proc_macro2::TokenStream, String> {
    let view_ident = ident(&view.name);
    let descriptor_ident = ident(&format!(
        "{}_VIEW",
        to_snake_case(&view.name).to_uppercase()
    ));
    let view_sql_name = to_snake_case(&view.name);

    // Primary key — empty string when `@@no_unique`. Validator already
    // rejected `@id` count != 1 unless `@@no_unique` set.
    let primary_key_sql = if view.no_unique() {
        String::new()
    } else {
        let pk_field = view
            .fields
            .iter()
            .find(|field| field.attributes.iter().any(|attr| attr.raw == "@id"))
            .ok_or_else(|| {
                format!(
                    "view `{}` has no @id field and is not @@no_unique (validator should have caught this)",
                    view.name
                )
            })?;
        to_snake_case(&pk_field.name)
    };

    // Resolve PK Rust type — used as the `PK` generic on
    // `ViewDescriptor<V, PK>`. For `@@no_unique` views we still need a
    // concrete type; `()` is the natural empty marker.
    let primary_key_type = if view.no_unique() {
        quote! { () }
    } else {
        let pk_field = view
            .fields
            .iter()
            .find(|field| field.attributes.iter().any(|attr| attr.raw == "@id"))
            .expect("validated view has @id when not @@no_unique");
        crate::shared::rust_type_tokens(&pk_field.ty)
    };

    // Columns — every view field becomes a `ModelColumn { rust_name,
    // sql_name }` entry. The user's CREATE VIEW SQL must produce
    // columns named with the `sql_name` (snake_case) so the per-row
    // decoder can `row.try_get(rust_name)` after the `select_projection`
    // emits `<sql_name> AS "<rust_name>"`.
    let columns: Vec<_> = view
        .fields
        .iter()
        .map(|field| {
            let rust_name = &field.name;
            let sql_name = to_snake_case(&field.name);
            quote! {
                ::cratestack::ModelColumn {
                    rust_name: #rust_name,
                    sql_name: #sql_name,
                }
            }
        })
        .collect();

    let allowed_fields: Vec<_> = view
        .fields
        .iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect();

    // Source tables — drives migration ordering. Sources are the
    // model names declared in `view <V> from <Model>, <Model>, ...`,
    // converted to snake_case for SQL identifiers.
    let source_tables: Vec<_> = view
        .sources
        .iter()
        .map(|source| {
            let name = crate::shared::pluralize(&to_snake_case(&source.name));
            quote! { #name }
        })
        .collect();

    let is_materialized = view.is_materialized();

    Ok(quote! {
        pub const #descriptor_ident: ::cratestack::ViewDescriptor<#view_ident, #primary_key_type> =
            ::cratestack::ViewDescriptor::new(
                "public",
                #view_sql_name,
                &[#(#columns),*],
                #primary_key_sql,
                &[#(#allowed_fields),*],
                &[#(#allowed_fields),*],
                &[],
                &[],
                &[],
                &[],
                #is_materialized,
                &[#(#source_tables),*],
            );
    })
}
