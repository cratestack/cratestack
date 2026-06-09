//! RPC dispatch arms emitted into the body of the generated
//! `rpc_dispatch` fn. Each model verb constructs a `ModelRouterState`
//! from the unified `RpcRouterState`, decodes the RPC body into the
//! right input shape, and delegates to the verb's `_dispatch` fn —
//! passing a `CanonicalRequest` describing the ACTUAL rpc request
//! (`POST /rpc/model.<M>.<verb>`, no query, the raw frame bytes) as the
//! canonical signed identity. On `transport rpc` that concrete rpc URL +
//! frame body is the single identity for url, dispatch, signing, and
//! tracing — it matches the rpc client byte-for-byte and the REST
//! `/<plural>` shape never appears.

use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::shared::{ident, is_primary_key, pluralize, rust_type_tokens, to_snake_case};

pub(crate) fn generate_procedure_rpc_dispatch_arm(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let op_id = format!("procedure.{}", procedure.name);
    let canonical_path = format!("/rpc/{op_id}");
    let dispatch_ident = ident(&format!(
        "handle_{}_dispatch",
        to_snake_case(&procedure.name)
    ));
    quote! {
        #op_id => {
            // The canonical signed request IS the actual rpc request:
            // `POST /rpc/procedure.<name>` with the raw frame bytes. This
            // matches the rpc client byte-for-byte; `/$procs/<name>` never
            // appears on `transport rpc`.
            let canonical_body = body.clone();
            #dispatch_ident(
                ProcedureRouterState {
                    db: state.db.clone(),
                    registry: state.registry.clone(),
                    codec: state.codec.clone(),
                    auth_provider: state.auth_provider.clone(),
                },
                CanonicalRequest {
                    method: "POST",
                    path: #canonical_path,
                    query: None,
                    body: canonical_body.as_ref(),
                },
                headers,
                body,
            ).await
        }
    }
}

/// Emit `model.<X>.{list,get,create,update,delete}` dispatch arms.
pub(crate) fn generate_model_rpc_dispatch_arms(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let m = model.name.as_str();
    let pk_field = model.fields.iter().find(|field| is_primary_key(field));
    let list_dispatch = ident(&format!(
        "handle_list_{}_dispatch",
        pluralize(&to_snake_case(m))
    ));
    let create_dispatch = ident(&format!(
        "handle_create_{}_dispatch",
        pluralize(&to_snake_case(m))
    ));
    let get_dispatch = ident(&format!("handle_get_{}_dispatch", to_snake_case(m)));
    let update_dispatch = ident(&format!("handle_update_{}_dispatch", to_snake_case(m)));
    let delete_dispatch = ident(&format!("handle_delete_{}_dispatch", to_snake_case(m)));
    let update_input_ident = ident(&format!("Update{m}Input"));

    let list_id = format!("model.{m}.list");
    let get_id = format!("model.{m}.get");
    let create_id = format!("model.{m}.create");
    let update_id = format!("model.{m}.update");
    let delete_id = format!("model.{m}.delete");

    // Concrete rpc URLs the client signs byte-for-byte: `/rpc/<op_id>`.
    let list_path = format!("/rpc/{list_id}");
    let get_path = format!("/rpc/{get_id}");
    let create_path = format!("/rpc/{create_id}");
    let update_path = format!("/rpc/{update_id}");
    let delete_path = format!("/rpc/{delete_id}");

    // Models without a primary key can't have get/update/delete ops
    // dispatch (no id to extract). The parser already rejects PK-less
    // models for REST; be defensive here too.
    let Some(pk) = pk_field else {
        return ["list", "get", "create", "update", "delete"]
            .into_iter()
            .map(|verb| {
                let op_id = format!("model.{m}.{verb}");
                quote! {
                    #op_id => {
                        rpc_dispatch_error(
                            &state,
                            &headers,
                            ::cratestack::CoolError::Internal(format!(
                                "model `{}` has no primary key; RPC dispatch impossible",
                                #m,
                            )),
                        )
                    }
                }
            })
            .collect();
    };
    let pk_type = rust_type_tokens(&pk.ty);

    vec![
        quote! {
            #list_id => {
                let model_state = ModelRouterState {
                    db: state.db.clone(),
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
                    raw_query,
                ).await
            }
        },
        quote! {
            #get_id => {
                let model_state = ModelRouterState {
                    db: state.db.clone(),
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
                #get_dispatch(
                    model_state,
                    CanonicalRequest {
                        method: "POST",
                        path: #get_path,
                        query: None,
                        body: body.as_ref(),
                    },
                    headers,
                    input.id,
                    None,
                ).await
            }
        },
        quote! {
            #create_id => {
                let model_state = ModelRouterState {
                    db: state.db.clone(),
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
                    body,
                ).await
            }
        },
        quote! {
            #update_id => {
                let model_state = ModelRouterState {
                    db: state.db.clone(),
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
                    input.id,
                    ::cratestack::axum::body::Bytes::from(patch_bytes),
                ).await
            }
        },
        quote! {
            #delete_id => {
                let model_state = ModelRouterState {
                    db: state.db.clone(),
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
                    input.id,
                ).await
            }
        },
    ]
}
