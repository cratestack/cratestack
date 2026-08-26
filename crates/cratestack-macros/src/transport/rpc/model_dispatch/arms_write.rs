//! `create`/`update`/`delete` RPC dispatch-arm builders — the three
//! write verbs, split out from the two read verbs (`arms_read`) to
//! keep each file under this crate's 200-LoC convention.

use quote::quote;

use super::ModelRpcContext;

pub(super) fn create_arm(ctx: &ModelRpcContext) -> proc_macro2::TokenStream {
    let m = &ctx.m;
    let create_id = format!("model.{m}.create");
    let create_path = format!("/rpc/{create_id}");
    let create_dispatch = &ctx.create_dispatch;
    quote! {
        #create_id => {
            let model_state = ModelRouterState {
                db: state.db.clone(),
                resolvers: state.resolvers.clone(),
                codec: state.codec.clone(),
                auth_provider: state.auth_provider.clone(),
            };
            let canonical_body = body.clone();
            #create_dispatch(
                model_state,
                CanonicalRequest {
                    method: "POST",
                    path: #create_path,
                    query: None,
                    body: canonical_body.as_ref(),
                },
                headers,
                client_ip_ctx,
                body,
            ).await
        }
    }
}

pub(super) fn update_arm(ctx: &ModelRpcContext) -> proc_macro2::TokenStream {
    let m = &ctx.m;
    let update_id = format!("model.{m}.update");
    let update_path = format!("/rpc/{update_id}");
    let update_dispatch = &ctx.update_dispatch;
    let update_input_ident = &ctx.update_input_ident;
    let pk_type = &ctx.pk_type;
    quote! {
        #update_id => {
            let model_state = ModelRouterState {
                db: state.db.clone(),
                resolvers: state.resolvers.clone(),
                codec: state.codec.clone(),
                auth_provider: state.auth_provider.clone(),
            };
            let input = match ::cratestack::__private::decode_rpc_body::<
                _,
                ::cratestack::rpc::RpcUpdateInput<#pk_type, super::inputs::#update_input_ident>,
            >(&state.codec, &headers, &body) {
                Ok(input) => input,
                Err(error) => return rpc_dispatch_error(&state, &headers, error),
            };
            let patch_bytes = match ::cratestack::__private::encode_rpc_value(
                &state.codec,
                &headers,
                &input.patch,
            ).await {
                Ok(bytes) => bytes,
                Err(error) => return rpc_dispatch_error(&state, &headers, error),
            };
            #update_dispatch(
                model_state,
                CanonicalRequest {
                    method: "POST",
                    path: #update_path,
                    // The full `{id, patch}` frame is the canonical body so
                    // both the id and the patch are bound to the signature;
                    // the re-encoded `patch` below is only the update logic's
                    // input, not the signed material.
                    query: None,
                    body: body.as_ref(),
                },
                headers,
                client_ip_ctx,
                input.id,
                ::cratestack::axum::body::Bytes::from(patch_bytes),
            ).await
        }
    }
}

pub(super) fn delete_arm(ctx: &ModelRpcContext) -> proc_macro2::TokenStream {
    let m = &ctx.m;
    let delete_id = format!("model.{m}.delete");
    let delete_path = format!("/rpc/{delete_id}");
    let delete_dispatch = &ctx.delete_dispatch;
    let pk_type = &ctx.pk_type;
    quote! {
        #delete_id => {
            let model_state = ModelRouterState {
                db: state.db.clone(),
                resolvers: state.resolvers.clone(),
                codec: state.codec.clone(),
                auth_provider: state.auth_provider.clone(),
            };
            let input = match ::cratestack::__private::decode_rpc_body::<
                _,
                ::cratestack::rpc::RpcPkInput<#pk_type>,
            >(&state.codec, &headers, &body) {
                Ok(input) => input,
                Err(error) => return rpc_dispatch_error(&state, &headers, error),
            };
            #delete_dispatch(
                model_state,
                CanonicalRequest {
                    method: "POST",
                    path: #delete_path,
                    query: None,
                    body: body.as_ref(),
                },
                headers,
                client_ip_ctx,
                input.id,
            ).await
        }
    }
}
