//! `ViewDescriptor` const emission.
//!
//! Mirrors [`crate::model::descriptor::generate_model_descriptor`] but
//! produces the narrower view-specific shape: no write policies, no
//! defaults/audit/version/PII state, no soft-delete column. Adds the
//! view-only fields `is_materialized` and `source_tables`.
//!
//! `@@allow("read", ...)` predicates are lowered through the existing
//! model policy machinery by synthesizing a `Model` from the view's
//! shared `attributes` + `fields` shape (see [`view_as_model`]). The
//! parser-level validator (see
//! `crates/cratestack-parser/src/validate/views.rs`) still rejects
//! any non-`"read"` action on a view, so the lowerer is fed a known-
//! good action set.

use cratestack_core::{Model, TypeDecl, View};
use quote::quote;

use crate::policy::{generate_denies_for_actions, generate_policies_for_actions};
use crate::shared::{ident, pluralize, to_snake_case};

pub(crate) fn generate_view_descriptor(
    view: &View,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let view_ident = ident(&view.name);
    let descriptor_ident = ident(&format!(
        "{}_VIEW",
        to_snake_case(&view.name).to_uppercase()
    ));
    // Same naming convention as tables (pluralize + snake_case) so
    // the view's SQL identifier matches what `cratestack-migrate`
    // emits in `CREATE VIEW`. `ActiveCustomer` → `active_customers`.
    let view_sql_name = pluralize(&to_snake_case(&view.name));

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

    // Lower the view's `@@allow("read", ...)` / `@@deny("read", ...)`
    // predicates through the existing model policy machinery. View
    // attributes carry the same shape as model attributes, so a
    // synthesized [`Model`] (see [`view_as_model`]) is the simplest
    // adapter — no duplication of the AST + relation-path lowerer.
    // Views only support the `"read"` action (validator-enforced), so
    // detail policies are the same set as read policies.
    let synthetic = view_as_model(view);
    let read_allow = generate_policies_for_actions(&synthetic, models, types, auth, &["read"])?;
    let read_deny = generate_denies_for_actions(&synthetic, models, types, auth, &["read"])?;
    let detail_allow = read_allow.clone();
    let detail_deny = read_deny.clone();

    Ok(quote! {
        pub const #descriptor_ident: ::cratestack::ViewDescriptor<#view_ident, #primary_key_type> =
            ::cratestack::ViewDescriptor::new(
                "public",
                #view_sql_name,
                &[#(#columns),*],
                #primary_key_sql,
                &[#(#allowed_fields),*],
                &[#(#allowed_fields),*],
                &[#(#read_allow),*],
                &[#(#read_deny),*],
                &[#(#detail_allow),*],
                &[#(#detail_deny),*],
                #is_materialized,
                &[#(#source_tables),*],
            );
    })
}

/// Synthesize a `Model` from a `View` for the policy lowerer's
/// consumption. The lowerer iterates `model.attributes` looking for
/// `@@allow` / `@@deny` directives — that surface is identical
/// between [`Model`] and [`View`], so a structurally-equivalent
/// `Model` is enough to reuse the entire AST + relation-path
/// pipeline. The lowerer also takes `model.fields` for predicate
/// field-name resolution; view fields are scalar (no relation paths
/// because the validator forbids them), so this is a straight
/// shallow clone.
fn view_as_model(view: &View) -> Model {
    Model {
        docs: view.docs.clone(),
        name: view.name.clone(),
        name_span: view.name_span,
        fields: view.fields.clone(),
        attributes: view.attributes.clone(),
        span: view.span,
    }
}
