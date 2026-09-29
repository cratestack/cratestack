//! `create`/`update`/`delete` method-group builders, split out of
//! `model.rs` for the 200-LoC file convention — mirrors
//! `transport::rpc::model_dispatch::arms_write`'s split (also
//! cratestack#743). Each builder is only called when the corresponding
//! verb survives `model_internal_actions` filtering; see `model.rs`'s
//! orchestrator for the call sites.

use quote::quote;

use super::context::ModelRestClientContext;
use super::routes::{bind_id, detail_runtime, detail_url, list_runtime};
use super::with_response::{build_delete_with_response_method, build_update_with_response_method};

pub(super) fn create_group(ctx: &ModelRestClientContext) -> proc_macro2::TokenStream {
    let route_path = &ctx.route_path;
    let model_output_type = &ctx.model_output_type;
    let create_input_ident = &ctx.create_input_ident;
    let runtime = list_runtime(route_path);
    quote! {
        pub async fn create(
            &self,
            input: &super::inputs::#create_input_ident,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
            #runtime.post(#route_path, input, headers).await
        }
    }
}

pub(super) fn update_group(ctx: &ModelRestClientContext) -> proc_macro2::TokenStream {
    let route_path = &ctx.route_path;
    let primary_key_type = &ctx.primary_key_type;
    let model_output_type = &ctx.model_output_type;
    let update_input_ident = &ctx.update_input_ident;
    let (bind_id, runtime, url) = (
        bind_id(),
        detail_runtime(route_path),
        detail_url(route_path),
    );
    let update_with_response_method = build_update_with_response_method(
        route_path,
        primary_key_type,
        update_input_ident,
        model_output_type,
    );
    quote! {
        pub async fn update(
            &self,
            id: &#primary_key_type,
            input: &super::inputs::#update_input_ident,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
            #bind_id
            #runtime.patch(#url, input, headers).await
        }

        #update_with_response_method
    }
}

pub(super) fn delete_group(ctx: &ModelRestClientContext) -> proc_macro2::TokenStream {
    let route_path = &ctx.route_path;
    let primary_key_type = &ctx.primary_key_type;
    let model_output_type = &ctx.model_output_type;
    let (bind_id, runtime, url) = (
        bind_id(),
        detail_runtime(route_path),
        detail_url(route_path),
    );
    let delete_with_response_method =
        build_delete_with_response_method(route_path, primary_key_type, model_output_type);
    quote! {
        pub async fn delete(
            &self,
            id: &#primary_key_type,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<#model_output_type, ::cratestack::client_rust::ClientError> {
            #bind_id
            #runtime.delete(#url, headers).await
        }

        #delete_with_response_method
    }
}
