//! RPC dispatch arms emitted into the body of the generated
//! `rpc_dispatch` fn. Each model verb constructs a `ModelRouterState`
//! from the unified `RpcRouterState`, decodes the RPC body into the
//! right input shape, synthesizes the axum extractor values the
//! existing CRUD handlers expect, and delegates.

use cratestack_core::{Model, Procedure};
use quote::quote;

use crate::shared::{ident, is_primary_key, pluralize, rust_type_tokens, to_snake_case};

pub(crate) fn generate_procedure_rpc_dispatch_arm(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let op_id = format!("procedure.{}", procedure.name);
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    quote! {
        #op_id => {
            #handler_ident(
                ::cratestack::axum::extract::State(ProcedureRouterState {
                    db: state.db.clone(),
                    registry: state.registry.clone(),
                    codec: state.codec.clone(),
                    auth_provider: state.auth_provider.clone(),
                }),
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
    let list_handler = ident(&format!("handle_list_{}", pluralize(&to_snake_case(m))));
    let create_handler = ident(&format!("handle_create_{}", pluralize(&to_snake_case(m))));
    let get_handler = ident(&format!("handle_get_{}", to_snake_case(m)));
    let update_handler = ident(&format!("handle_update_{}", to_snake_case(m)));
    let delete_handler = ident(&format!("handle_delete_{}", to_snake_case(m)));
    let update_input_ident = ident(&format!("Update{m}Input"));

    let list_id = format!("model.{m}.list");
    let get_id = format!("model.{m}.get");
    let create_id = format!("model.{m}.create");
    let update_id = format!("model.{m}.update");
    let delete_id = format!("model.{m}.delete");

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
                #list_handler(
                    ::cratestack::axum::extract::State(model_state),
                    headers,
                    ::cratestack::axum::extract::RawQuery(raw_query),
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
                #get_handler(
                    ::cratestack::axum::extract::State(model_state),
                    headers,
                    ::cratestack::axum::extract::Path(input.id),
                    ::cratestack::axum::extract::RawQuery(None),
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
                #create_handler(
                    ::cratestack::axum::extract::State(model_state),
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
                #update_handler(
                    ::cratestack::axum::extract::State(model_state),
                    headers,
                    ::cratestack::axum::extract::Path(input.id),
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
                #delete_handler(
                    ::cratestack::axum::extract::State(model_state),
                    headers,
                    ::cratestack::axum::extract::Path(input.id),
                ).await
            }
        },
    ]
}
