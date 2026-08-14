//! `build_get_arm`/`build_delete_arm`/`build_create_arm`/`build_update_arm`
//! — four of the five per-model CRUD match arms `service::build_service`
//! splices into `ApiServer::call` (ticket #171); the fifth,
//! `build_list_arm`, lives in `crud_arm_list.rs` (its paged/unpaged
//! branch makes it long enough on its own to want the split). All five
//! decode a pb request, dispatch through the exact same `super::axum::
//! handle_*_dispatch` fn REST/RPC already call, and bridge the result
//! back through `bridge_grpc_response` — see `service.rs`'s module doc
//! for the full shape and the "known gap" this shares with the procedure
//! arms in `procedure_arms.rs`.
//!
//! Each function below supplies only what actually differs between CRUD
//! verbs — which dispatch fn, how many/which arguments it takes, and how
//! the decoded pb request becomes those arguments — as an
//! [`crud_arm_spec::ArmSpec`], and hands it to [`crud_arm_spec::
//! build_unary_arm`], the one place the shared marker-struct/`impl
//! UnaryService`/`Box::pin` shape gets built (cratestack#426 — these five
//! used to each reimplement that shape independently).

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

use super::arm_support::status_from_bridge_error;
use super::crud_arm_spec::{ArmSpec, build_unary_arm, method_path};

pub(super) fn build_get_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Get");
    let dispatch_ident = ident(&format!(
        "handle_get_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcPkInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}GetSvc", model.name));
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let body = quote! {
        let id = message.into_pk().map_err(|error| {
            ::cratestack::grpc::tonic::Status::new(
                ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                error.public_message().into_owned(),
            )
        })?;
        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, id, None).await;
        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
            Ok(value) => value,
            Err((code, message)) => return Err(#status),
        };
        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
    };
    build_unary_arm(ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    })
}

pub(super) fn build_delete_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Delete");
    let dispatch_ident = ident(&format!(
        "handle_delete_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcPkInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}DeleteSvc", model.name));
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let body = quote! {
        let id = message.into_pk().map_err(|error| {
            ::cratestack::grpc::tonic::Status::new(
                ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                error.public_message().into_owned(),
            )
        })?;
        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, id).await;
        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
            Ok(value) => value,
            Err((code, message)) => return Err(#status),
        };
        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
    };
    build_unary_arm(ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    })
}

pub(super) fn build_create_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Create");
    let dispatch_ident = ident(&format!(
        "handle_create_{}_dispatch",
        pluralize(&to_snake_case(&model.name))
    ));
    let request_ty = ident(&format!("Create{}Input", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}CreateSvc", model.name));
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let body = quote! {
        let domain: ::core::result::Result<super::#request_ty, ::cratestack::CratestackError> =
            super::#request_ty::try_from(message);
        let domain = match domain {
            Ok(value) => value,
            Err(error) => {
                return Err(::cratestack::grpc::tonic::Status::new(
                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                    error.public_message().into_owned(),
                ));
            }
        };
        let body_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &domain).await {
            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
            Err(error) => {
                return Err(::cratestack::grpc::tonic::Status::new(
                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                    error.public_message().into_owned(),
                ));
            }
        };
        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, body_bytes).await;
        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
            Ok(value) => value,
            Err((code, message)) => return Err(#status),
        };
        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
    };
    build_unary_arm(ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    })
}

pub(super) fn build_update_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "Update");
    let dispatch_ident = ident(&format!(
        "handle_update_{}_dispatch",
        to_snake_case(&model.name)
    ));
    let request_ty = ident(&format!("{}RpcUpdateInput", model.name));
    let response_ty = ident(&model.name);
    let svc_ident = ident(&format!("Grpc{}UpdateSvc", model.name));
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    let body = quote! {
        let (id, patch) = match message.into_id_and_patch() {
            Ok(value) => value,
            Err(error) => {
                return Err(::cratestack::grpc::tonic::Status::new(
                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                    error.public_message().into_owned(),
                ));
            }
        };
        let patch_bytes = match ::cratestack::__private::encode_rpc_value(&state.codec, &headers, &patch).await {
            Ok(bytes) => ::cratestack::axum::body::Bytes::from(bytes),
            Err(error) => {
                return Err(::cratestack::grpc::tonic::Status::new(
                    ::cratestack::grpc::cool_error_code_to_tonic_code(error.code()),
                    error.public_message().into_owned(),
                ));
            }
        };
        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), client_ip_ctx, id, patch_bytes).await;
        let domain: super::#response_ty = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
            Ok(value) => value,
            Err((code, message)) => return Err(#status),
        };
        Ok(::cratestack::grpc::tonic::Response::new(pb::#response_ty::from(&domain)))
    };
    build_unary_arm(ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    })
}
