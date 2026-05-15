use cratestack_core::{Model, Procedure, ProcedureKind, TypeArity};
use quote::quote;

use crate::shared::{ident, is_primary_key, pluralize, rust_type_tokens, to_snake_case};

pub(crate) fn generate_procedure_transport_constants(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let const_ident = route_transport_const_ident("procedure", &procedure.name, "post");
    let path = format!("/$procs/{}", procedure.name);
    let capabilities = procedure_transport_capabilities_tokens(procedure);
    let name = procedure.name.as_str();

    Ok(quote! {
        pub const #const_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #name,
            method: "POST",
            path: #path,
            capabilities: #capabilities,
        };
    })
}

pub(crate) fn generate_procedure_transport_entries(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    let const_ident = route_transport_const_ident("procedure", &procedure.name, "post");
    quote! { #const_ident }
}

pub(crate) fn generate_model_transport_constants(model: &Model) -> proc_macro2::TokenStream {
    let model_name = &model.name;
    let list_path = format!("/{}", pluralize(&to_snake_case(model_name)));
    let detail_path = format!("/{}/{{id}}", pluralize(&to_snake_case(model_name)));

    let list_ident = route_transport_const_ident("model", model_name, "list_get");
    let create_ident = route_transport_const_ident("model", model_name, "list_post");
    let get_ident = route_transport_const_ident("model", model_name, "detail_get");
    let update_ident = route_transport_const_ident("model", model_name, "detail_patch");
    let delete_ident = route_transport_const_ident("model", model_name, "detail_delete");

    let read_caps = model_read_transport_capabilities_tokens();
    let write_caps = model_write_transport_capabilities_tokens();

    quote! {
        pub const #list_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "GET",
            path: #list_path,
            capabilities: #read_caps,
        };

        pub const #create_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "POST",
            path: #list_path,
            capabilities: #write_caps,
        };

        pub const #get_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "GET",
            path: #detail_path,
            capabilities: #read_caps,
        };

        pub const #update_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "PATCH",
            path: #detail_path,
            capabilities: #write_caps,
        };

        pub const #delete_ident: ::cratestack::RouteTransportDescriptor = ::cratestack::RouteTransportDescriptor {
            name: #model_name,
            method: "DELETE",
            path: #detail_path,
            capabilities: #read_caps,
        };
    }
}

pub(crate) fn generate_model_transport_entries(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let model_name = &model.name;
    let list_ident = route_transport_const_ident("model", model_name, "list_get");
    let create_ident = route_transport_const_ident("model", model_name, "list_post");
    let get_ident = route_transport_const_ident("model", model_name, "detail_get");
    let update_ident = route_transport_const_ident("model", model_name, "detail_patch");
    let delete_ident = route_transport_const_ident("model", model_name, "detail_delete");

    vec![
        quote! { #list_ident },
        quote! { #create_ident },
        quote! { #get_ident },
        quote! { #update_ident },
        quote! { #delete_ident },
    ]
}

pub(crate) fn route_transport_const_ident(kind: &str, name: &str, suffix: &str) -> syn::Ident {
    ident(&format!("{}_{}_{}", kind, to_snake_case(name), suffix).to_ascii_uppercase())
}

pub(crate) fn procedure_transport_capabilities_tokens(
    procedure: &Procedure,
) -> proc_macro2::TokenStream {
    if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &[
                    "application/cbor",
                    "application/json",
                    ::cratestack::CBOR_SEQUENCE_CONTENT_TYPE,
                ],
                default_response_type: "application/cbor",
                supports_sequence_response: true,
            }
        }
    } else {
        quote! {
            ::cratestack::RouteTransportCapabilities {
                request_types: &["application/cbor", "application/json"],
                response_types: &["application/cbor", "application/json"],
                default_response_type: "application/cbor",
                supports_sequence_response: false,
            }
        }
    }
}

pub(crate) fn model_read_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &[],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}

pub(crate) fn model_write_transport_capabilities_tokens() -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::RouteTransportCapabilities {
            request_types: &["application/cbor", "application/json"],
            response_types: &["application/cbor", "application/json"],
            default_response_type: "application/cbor",
            supports_sequence_response: false,
        }
    }
}

// -----------------------------------------------------------------------------
// RPC op descriptors
//
// These are emitted in addition to the REST `RouteTransportDescriptor` consts
// above. The top-level macro chooses which slice (`ROUTE_TRANSPORTS` or `OPS`)
// to populate based on `Schema.transport`; both consts always exist on the
// generated module so downstream code compiles uniformly.
//
// See `docs/design/rpc-transport.md` for the semantic spec.
//
// `auth_required` is currently a placeholder — set to `true` whenever the
// schema declares an `auth` block, `false` otherwise. Per-op policy resolution
// (parsing `@allow` / `@@allow` attributes) is future work; consumers should
// treat this field as advisory until then.
// -----------------------------------------------------------------------------

pub(crate) fn generate_model_op_descriptors(
    model: &Model,
    auth_required: bool,
) -> Vec<proc_macro2::TokenStream> {
    let model_name = model.name.as_str();
    let page_ty = format!("Page<{model_name}>");
    let create_input = format!("Create{model_name}Input");
    let update_input = format!("Update{model_name}Input");

    let list_id = format!("model.{model_name}.list");
    let get_id = format!("model.{model_name}.get");
    let create_id = format!("model.{model_name}.create");
    let update_id = format!("model.{model_name}.update");
    let delete_id = format!("model.{model_name}.delete");

    vec![
        quote! {
            ::cratestack::OpDescriptor {
                op_id: #list_id,
                kind: ::cratestack::OpKind::Unary,
                input_ty: "",
                output_ty: #page_ty,
                idempotent_by_default: true,
                auth_required: #auth_required,
            }
        },
        quote! {
            ::cratestack::OpDescriptor {
                op_id: #get_id,
                kind: ::cratestack::OpKind::Unary,
                input_ty: "",
                output_ty: #model_name,
                idempotent_by_default: true,
                auth_required: #auth_required,
            }
        },
        quote! {
            ::cratestack::OpDescriptor {
                op_id: #create_id,
                kind: ::cratestack::OpKind::Unary,
                input_ty: #create_input,
                output_ty: #model_name,
                idempotent_by_default: false,
                auth_required: #auth_required,
            }
        },
        quote! {
            ::cratestack::OpDescriptor {
                op_id: #update_id,
                kind: ::cratestack::OpKind::Unary,
                input_ty: #update_input,
                output_ty: #model_name,
                idempotent_by_default: false,
                auth_required: #auth_required,
            }
        },
        quote! {
            ::cratestack::OpDescriptor {
                op_id: #delete_id,
                kind: ::cratestack::OpKind::Unary,
                input_ty: "",
                output_ty: #model_name,
                idempotent_by_default: false,
                auth_required: #auth_required,
            }
        },
    ]
}

// -----------------------------------------------------------------------------
// RPC dispatch arms
//
// Emitted into the body of the generated `rpc_dispatch` fn — one arm per
// callable. Procedures delegate to the existing axum handler (already shaped
// as `(State, HeaderMap, Bytes) -> Response`). Model CRUD verbs delegate to
// a per-verb adapter that builds the right axum extractor values from the
// RPC request body — this lives in a future patch, so for now the macro
// emits arms that return a 501-shaped CoolError::Internal explaining the
// gap.
// -----------------------------------------------------------------------------

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
///
/// Each arm constructs a `ModelRouterState` from the unified
/// `RpcRouterState`, decodes the RPC body into the right input shape
/// from `cratestack::rpc`, synthesizes the axum extractor values the
/// existing CRUD handlers expect (`Path(id)`, `RawQuery(qs)`, `Bytes`),
/// and delegates. The handlers themselves are untouched, so REST and
/// RPC share one code path per verb.
pub(crate) fn generate_model_rpc_dispatch_arms(model: &Model) -> Vec<proc_macro2::TokenStream> {
    let m = model.name.as_str();
    let pk_field = model
        .fields
        .iter()
        .find(|field| is_primary_key(field));
    let list_handler =
        ident(&format!("handle_list_{}", pluralize(&to_snake_case(m))));
    let create_handler =
        ident(&format!("handle_create_{}", pluralize(&to_snake_case(m))));
    let get_handler = ident(&format!("handle_get_{}", to_snake_case(m)));
    let update_handler = ident(&format!("handle_update_{}", to_snake_case(m)));
    let delete_handler = ident(&format!("handle_delete_{}", to_snake_case(m)));
    let update_input_ident = ident(&format!("Update{m}Input"));

    let list_id = format!("model.{m}.list");
    let get_id = format!("model.{m}.get");
    let create_id = format!("model.{m}.create");
    let update_id = format!("model.{m}.update");
    let delete_id = format!("model.{m}.delete");

    // Models without a primary key can't have `get`/`update`/`delete` ops
    // dispatch (no id to extract). The parser already rejects PK-less
    // models for the REST binding, but be defensive here too — emit a
    // dispatch arm that returns 500 if somehow reached.
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
        // model.<X>.list — decode RpcListInput, synthesize query string,
        // call existing list handler with RawQuery(Some(qs)).
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
        // model.<X>.get — decode RpcPkInput<Pk>, construct Path(id), call.
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
        // model.<X>.create — body shape already matches the REST POST.
        // Delegate directly; no decode-then-re-encode.
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
        // model.<X>.update — decode {id, patch} with the patch typed to
        // the model's concrete `Update<X>Input` (so CBOR Option::None
        // round-trips correctly), re-encode just the patch via the same
        // codec, then call existing update handler with Path(id) + Bytes.
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
        // model.<X>.delete — decode {id}, call existing delete handler
        // with Path(id).
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

pub(crate) fn generate_procedure_op_descriptor(
    procedure: &Procedure,
    auth_required: bool,
) -> proc_macro2::TokenStream {
    let op_id = format!("procedure.{}", procedure.name);
    let kind = if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! { ::cratestack::OpKind::Sequence }
    } else {
        quote! { ::cratestack::OpKind::Unary }
    };
    // For now, the input type is the first arg's type name (the conventional
    // single-`args` arg). Procedures with zero or multiple args expose an
    // empty `input_ty`; richer surfacing is future work.
    let input_ty = procedure
        .args
        .first()
        .map(|a| a.ty.name.as_str())
        .unwrap_or("");
    let output_ty = procedure.return_type.name.as_str();
    // Queries are safe to retry without an idempotency key; mutations are not.
    let idempotent = matches!(procedure.kind, ProcedureKind::Query);

    quote! {
        ::cratestack::OpDescriptor {
            op_id: #op_id,
            kind: #kind,
            input_ty: #input_ty,
            output_ty: #output_ty,
            idempotent_by_default: #idempotent,
            auth_required: #auth_required,
        }
    }
}
