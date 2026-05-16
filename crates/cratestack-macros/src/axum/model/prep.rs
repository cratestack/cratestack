//! Per-model identifier names + reusable token chunks consumed by
//! the handler/builder emitters. Materialized once per
//! `generate_model_axum_handlers` call so the orchestrator stays flat.

mod etag;
mod list_logging;

use cratestack_core::Model;
use quote::quote;

use crate::shared::{
    ident, is_paged_model, is_primary_key, pluralize, rust_type_tokens, to_snake_case,
};
use crate::transport::{
    model_read_transport_capabilities_tokens, model_write_transport_capabilities_tokens,
};

use super::super::policy_attr::create_requires_authenticated_context;

pub(super) struct ModelHandlerPrep {
    pub(super) list_handler_ident: syn::Ident,
    pub(super) create_handler_ident: syn::Ident,
    pub(super) get_handler_ident: syn::Ident,
    pub(super) update_handler_ident: syn::Ident,
    pub(super) delete_handler_ident: syn::Ident,
    pub(super) model_ident: syn::Ident,
    pub(super) field_module_ident: syn::Ident,
    pub(super) accessor_ident: syn::Ident,
    pub(super) model_name: String,
    pub(super) list_route_path: String,
    pub(super) create_input_ident: syn::Ident,
    pub(super) update_input_ident: syn::Ident,
    pub(super) list_builder_ident: syn::Ident,
    pub(super) validate_selection_ident: syn::Ident,
    pub(super) validate_include_path_ident: syn::Ident,
    pub(super) validate_include_fields_path_ident: syn::Ident,
    pub(super) project_model_value_ident: syn::Ident,
    pub(super) project_object_fields_ident: syn::Ident,
    pub(super) project_serialized_value_ident: syn::Ident,
    pub(super) serialize_model_value_ident: syn::Ident,
    pub(super) filter_expr_builder_ident: syn::Ident,
    pub(super) query_expr_builder_ident: syn::Ident,
    pub(super) list_capabilities: proc_macro2::TokenStream,
    pub(super) write_capabilities: proc_macro2::TokenStream,
    pub(super) detail_capabilities: proc_macro2::TokenStream,
    pub(super) paged: bool,
    pub(super) primary_key_type: proc_macro2::TokenStream,
    pub(super) list_response_type: proc_macro2::TokenStream,
    pub(super) list_header_error_encoder: proc_macro2::TokenStream,
    pub(super) create_auth_preflight: proc_macro2::TokenStream,
    pub(super) update_empty_patch_preflight: proc_macro2::TokenStream,
    pub(super) update_if_match_decl: proc_macro2::TokenStream,
    pub(super) update_if_match_apply: proc_macro2::TokenStream,
    pub(super) update_etag_extract: proc_macro2::TokenStream,
    pub(super) update_etag_apply: proc_macro2::TokenStream,
    pub(super) get_etag_extract_decl: proc_macro2::TokenStream,
    pub(super) get_etag_capture: proc_macro2::TokenStream,
    pub(super) get_etag_apply: proc_macro2::TokenStream,
    pub(super) total_count_block: proc_macro2::TokenStream,
    pub(super) list_success_value: proc_macro2::TokenStream,
    pub(super) list_result_log: proc_macro2::TokenStream,
}

pub(super) fn build_prep(model: &Model) -> Result<ModelHandlerPrep, String> {
    let version_field_name: Option<String> = model
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|a| a.raw == "@version"))
        .map(|field| field.name.clone());
    let snake = to_snake_case(&model.name);
    let plural = pluralize(&snake);
    let model_ident = ident(&model.name);
    let create_input_ident = ident(&format!("Create{}Input", model.name));
    let update_input_ident = ident(&format!("Update{}Input", model.name));
    let list_route_path = format!("/{}", plural);

    let paged = is_paged_model(model);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let list_response_type = if paged {
        quote! { ::cratestack::Page<::cratestack::serde_json::Value> }
    } else {
        quote! { Vec<::cratestack::serde_json::Value> }
    };
    let list_header_error_encoder = if paged {
        quote! { ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::Page<::cratestack::serde_json::Value>>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error)) }
    } else {
        quote! { ::cratestack::encode_transport_result_with_status_for::<_, Vec<::cratestack::serde_json::Value>>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error)) }
    };
    let create_auth_preflight = if create_requires_authenticated_context(model) {
        quote! {
            if !ctx.is_authenticated() {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                    &state.codec,
                    &headers,
                    &CAPABILITIES,
                    axum::http::StatusCode::OK,
                    Err(CoolError::Forbidden("create policy denied this operation".to_owned())),
                );
            }
        }
    } else {
        quote! {}
    };
    let update_empty_patch_preflight = quote! {
        if <super::inputs::#update_input_ident as ::cratestack::UpdateModelInput<super::models::#model_ident>>::sql_values(&input).is_empty() {
            return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                &state.codec,
                &headers,
                &CAPABILITIES,
                axum::http::StatusCode::OK,
                Err(CoolError::Validation("update input must contain at least one changed column".to_owned())),
            );
        }
    };
    let etag = etag::etag_tokens(&version_field_name, &model_ident);
    let list_builder_ident = ident(&format!("build_{}_list_request", snake));
    let total_count_block =
        list_logging::total_count_tokens(paged, &list_builder_ident, &list_response_type);
    let list_success_value = list_logging::list_success_tokens(paged);
    let list_result_log = list_logging::list_result_log_tokens(paged, &list_route_path, &model.name);

    Ok(ModelHandlerPrep {
        list_handler_ident: ident(&format!("handle_list_{}", plural)),
        create_handler_ident: ident(&format!("handle_create_{}", plural)),
        get_handler_ident: ident(&format!("handle_get_{}", snake)),
        update_handler_ident: ident(&format!("handle_update_{}", snake)),
        delete_handler_ident: ident(&format!("handle_delete_{}", snake)),
        model_ident,
        field_module_ident: ident(&snake),
        accessor_ident: ident(&snake),
        model_name: model.name.clone(),
        list_route_path,
        create_input_ident,
        update_input_ident,
        list_builder_ident,
        validate_selection_ident: ident(&format!("validate_{}_selection_query", snake)),
        validate_include_path_ident: ident(&format!("validate_{}_include_path", snake)),
        validate_include_fields_path_ident: ident(&format!("validate_{}_include_fields_path", snake)),
        project_model_value_ident: ident(&format!("project_{}_model_value", snake)),
        project_object_fields_ident: ident(&format!("project_{}_object_fields", snake)),
        project_serialized_value_ident: ident(&format!("project_{}_serialized_value", snake)),
        serialize_model_value_ident: ident(&format!("serialize_{}_model_value", snake)),
        filter_expr_builder_ident: ident(&format!("build_{}_filter_expr", snake)),
        query_expr_builder_ident: ident(&format!("build_{}_query_expr", snake)),
        list_capabilities: model_read_transport_capabilities_tokens(),
        write_capabilities: model_write_transport_capabilities_tokens(),
        detail_capabilities: model_read_transport_capabilities_tokens(),
        paged,
        primary_key_type,
        list_response_type,
        list_header_error_encoder,
        create_auth_preflight,
        update_empty_patch_preflight,
        update_if_match_decl: etag.update_if_match_decl,
        update_if_match_apply: etag.update_if_match_apply,
        update_etag_extract: etag.update_etag_extract,
        update_etag_apply: etag.update_etag_apply,
        get_etag_extract_decl: etag.get_etag_extract_decl,
        get_etag_capture: etag.get_etag_capture,
        get_etag_apply: etag.get_etag_apply,
        total_count_block,
        list_success_value,
        list_result_log,
    })
}
