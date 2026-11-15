//! `ModelDescriptor` const emission — the static metadata table the
//! runtime consults at every CRUD/list call (columns, allowed
//! field/include/sort sets, all 10 policy slots, auth-derived defaults,
//! emitted events, audit/PII/sensitive/soft-delete flags, upsert
//! columns). One per model; lives in `pub mod models { ... }`.

mod columns;
mod defaults;
mod embedded;
mod generated;
mod unique;

use cratestack_core::{EnumDecl, Field, Model, TypeDecl};
use quote::quote;

use crate::event::model_emitted_events;
use crate::policy::{
    audit_model_policies, generate_denies_for_actions, generate_policies_for_actions,
};
use crate::shared::{ident, is_primary_key, rust_type_tokens, to_snake_case};

use columns::collect_column_lists;
use defaults::collect_create_defaults;
use embedded::collect_embedded_defaults;
use generated::collect_generated_defaults;
use unique::unique_column_names;

pub(crate) fn generate_model_descriptor(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    enums: &[EnumDecl],
    auth: Option<&cratestack_core::AuthBlock>,
    embedded: bool,
) -> Result<proc_macro2::TokenStream, String> {
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let table_name = model.sql_table_name();
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let primary_key_sql = primary_key.sql_column_name();

    // GHSA-69g4-xvcm-vm2j: every slot below, so a rule no slot takes is an error.
    let owner = format!("model `{}`", model.name);
    let slots = ["list", "read", "detail", "create", "update", "delete"];
    audit_model_policies(&owner, model, &slots)?;
    // The slot each verb reads is `cratestack_policy::model_verb_policy_slot`'s
    // table, shared with `cratestack contract ops` (cratestack#1388).
    use cratestack_policy::ModelPolicySlot::{Create, Delete, Detail, List, Update};
    let allow = |slot: cratestack_policy::ModelPolicySlot| {
        generate_policies_for_actions(model, models, types, enums, auth, slot.actions())
    };
    let deny = |slot: cratestack_policy::ModelPolicySlot| {
        generate_denies_for_actions(model, models, types, enums, auth, slot.actions())
    };
    let (read_policies, read_deny_policies) = (allow(List)?, deny(List)?);
    let (detail_policies, detail_deny_policies) = (allow(Detail)?, deny(Detail)?);
    let (create_policies, create_deny_policies) = (allow(Create)?, deny(Create)?);
    let (update_policies, update_deny_policies) = (allow(Update)?, deny(Update)?);
    let (delete_policies, delete_deny_policies) = (allow(Delete)?, deny(Delete)?);

    let create_defaults = collect_create_defaults(model, models, types, auth)?;
    let generated_defaults = collect_generated_defaults(model, models);
    let embedded_defaults = if embedded {
        collect_embedded_defaults(model, models, enums)?
    } else {
        quote! {}
    };
    let emitted_events = emitted_event_tokens(model)?;
    let cols = collect_column_lists(model, models)?;

    let version_column_tokens = match version_field(model) {
        Some(field) => {
            let column = field.sql_column_name();
            quote! { Some(#column) }
        }
        None => quote! { None },
    };
    let audit_enabled = model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@audit");
    let soft_delete_enabled = model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@soft_delete");
    let soft_delete_column_tokens = if soft_delete_enabled {
        quote! { Some("deleted_at") }
    } else {
        quote! { None }
    };
    let retention_days_tokens = model
        .attributes
        .iter()
        .find_map(|attribute| {
            attribute
                .raw
                .strip_prefix("@@retain(days:")
                .and_then(|rest| rest.strip_suffix(')'))
                .map(str::trim)
                .and_then(|raw| raw.parse::<u32>().ok())
        })
        .map(|n| quote! { Some(#n) })
        .unwrap_or_else(|| quote! { None });

    let columns = cols.columns;
    let allowed_fields = cols.allowed_fields;
    let allowed_includes = cols.allowed_includes;
    let allowed_sorts = cols.allowed_sorts;
    let pii_columns = cols.pii_columns;
    let sensitive_columns = cols.sensitive_columns;
    let upsert_update_columns = cols.upsert_update_columns;

    // `@@subscribe`: the model's events are also notified to subscribers.
    let subscribed = cratestack_core::model_verbs(model)
        .contains(&cratestack_core::ModelVerb::Subscribe)
        .then(|| quote! { .with_subscribed() });

    let unique_columns = unique_column_names(model);
    let unique = (!unique_columns.is_empty())
        .then(|| quote! { .with_unique_columns(&[#(#unique_columns),*]) });

    Ok(quote! {
        pub const #descriptor_ident: ::cratestack::ModelDescriptor<#model_ident, #primary_key_type> =
            ::cratestack::ModelDescriptor::new(
                stringify!(#model_ident),
                #table_name,
                &[#(#columns),*],
                #primary_key_sql,
                &[#(#allowed_fields),*],
                &[#(#allowed_includes),*],
                &[#(#allowed_sorts),*],
                &[#(#read_policies),*],
                &[#(#read_deny_policies),*],
                &[#(#detail_policies),*],
                &[#(#detail_deny_policies),*],
                &[#(#create_policies),*],
                &[#(#create_deny_policies),*],
                &[#(#update_policies),*],
                &[#(#update_deny_policies),*],
                &[#(#delete_policies),*],
                &[#(#delete_deny_policies),*],
                &[#(#create_defaults),*],
                &[#(#emitted_events),*],
                #version_column_tokens,
                #audit_enabled,
                &[#(#pii_columns),*],
                &[#(#sensitive_columns),*],
                #soft_delete_column_tokens,
                #retention_days_tokens,
                &[#(#upsert_update_columns),*],
            )
            .with_generated_defaults(&[#(#generated_defaults),*])
            #embedded_defaults
            #subscribed
            #unique;
    })
}

fn emitted_event_tokens(model: &Model) -> Result<Vec<proc_macro2::TokenStream>, String> {
    Ok(model_emitted_events(model)?
        .into_iter()
        .map(|operation| match operation {
            cratestack_core::ModelEventKind::Created => {
                quote! { ::cratestack::ModelEventKind::Created }
            }
            cratestack_core::ModelEventKind::Updated => {
                quote! { ::cratestack::ModelEventKind::Updated }
            }
            cratestack_core::ModelEventKind::Deleted => {
                quote! { ::cratestack::ModelEventKind::Deleted }
            }
        })
        .collect())
}

pub(super) fn version_field(model: &Model) -> Option<&Field> {
    model
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|a| a.raw == "@version"))
}
