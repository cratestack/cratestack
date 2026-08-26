//! `list`/`get` method-group builders, split out of `model.rs` for the
//! 200-LoC file convention — mirrors
//! `transport::rpc::model_dispatch::arms_read`'s split (also
//! cratestack#743). Each builder is only called when the corresponding
//! verb survives `model_internal_actions` filtering; see `model.rs`'s
//! orchestrator for the call sites.

use quote::quote;

use super::computed::{build_get_method, build_list_method};
use super::context::ModelRestClientContext;
use super::with_response::build_get_with_response_method;

pub(super) fn list_group(ctx: &ModelRestClientContext) -> proc_macro2::TokenStream {
    let route_path = &ctx.route_path;
    let list_output_type = &ctx.list_output_type;
    let list_view_output_type = &ctx.list_view_output_type;
    let list_view_call = &ctx.list_view_call;
    let list_method = build_list_method(
        ctx.computed_params_ident.as_ref(),
        route_path,
        list_output_type,
    );
    quote! {
        #list_method

        pub async fn list_view<P>(
            &self,
            projection: &P,
            query: &[::cratestack::client_rust::QueryPair<'_>],
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<#list_view_output_type, ::cratestack::client_rust::ClientError>
        where
            P: ::cratestack::ProjectionDecoder,
        {
            #list_view_call
        }
    }
}

pub(super) fn get_group(ctx: &ModelRestClientContext) -> proc_macro2::TokenStream {
    let route_path = &ctx.route_path;
    let primary_key_type = &ctx.primary_key_type;
    let model_output_type = &ctx.model_output_type;
    let get_method = build_get_method(
        ctx.computed_params_ident.as_ref(),
        route_path,
        primary_key_type,
        model_output_type,
    );
    let get_with_response_method =
        build_get_with_response_method(route_path, primary_key_type, model_output_type);
    quote! {
        #get_method

        #get_with_response_method

        pub async fn get_view<P>(
            &self,
            id: &#primary_key_type,
            projection: &P,
            headers: &[::cratestack::client_rust::HeaderPair<'_>],
        ) -> Result<P::Output, ::cratestack::client_rust::ClientError>
        where
            P: ::cratestack::ProjectionDecoder,
        {
            self.runtime
                .get_view(&format!("{}/{}", #route_path, id), projection, headers)
                .await
        }
    }
}
