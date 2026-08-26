//! `list`/`get` RPC dispatch-arm builders — the two read verbs, split
//! out from the three write verbs (`arms_write`) to keep each file
//! under this crate's 200-LoC convention.

use quote::quote;

use super::ModelRpcContext;

pub(super) fn list_arm(ctx: &ModelRpcContext) -> proc_macro2::TokenStream {
    let m = &ctx.m;
    let list_id = format!("model.{m}.list");
    let list_path = format!("/rpc/{list_id}");
    let list_dispatch = &ctx.list_dispatch;
    quote! {
        #list_id => {
            let model_state = ModelRouterState {
                db: state.db.clone(),
                resolvers: state.resolvers.clone(),
                codec: state.codec.clone(),
                auth_provider: state.auth_provider.clone(),
            };
            let input = match ::cratestack::__private::decode_rpc_body::<
                _,
                ::cratestack::rpc::RpcListInput,
            >(&state.codec, &headers, &body) {
                Ok(input) => input,
                Err(error) => return rpc_dispatch_error(&state, &headers, error),
            };
            let raw_query = ::cratestack::rpc::synthesize_list_query(&input);
            #list_dispatch(
                model_state,
                CanonicalRequest {
                    method: "POST",
                    path: #list_path,
                    query: None,
                    body: body.as_ref(),
                },
                headers,
                client_ip_ctx,
                raw_query,
            ).await
        }
    }
}

pub(super) fn get_arm(ctx: &ModelRpcContext) -> proc_macro2::TokenStream {
    let m = &ctx.m;
    let get_id = format!("model.{m}.get");
    let get_path = format!("/rpc/{get_id}");
    let get_dispatch = &ctx.get_dispatch;
    let pk_type = &ctx.pk_type;
    quote! {
        #get_id => {
            let model_state = ModelRouterState {
                db: state.db.clone(),
                resolvers: state.resolvers.clone(),
                codec: state.codec.clone(),
                auth_provider: state.auth_provider.clone(),
            };
            let input = match ::cratestack::__private::decode_rpc_body::<
                _,
                ::cratestack::rpc::RpcGetInput<#pk_type>,
            >(&state.codec, &headers, &body) {
                Ok(input) => input,
                Err(error) => return rpc_dispatch_error(&state, &headers, error),
            };
            let raw_query = ::cratestack::rpc::synthesize_get_query(&input);
            #get_dispatch(
                model_state,
                CanonicalRequest {
                    method: "POST",
                    path: #get_path,
                    query: None,
                    body: body.as_ref(),
                },
                headers,
                client_ip_ctx,
                input.id,
                raw_query,
            ).await
        }
    }
}
