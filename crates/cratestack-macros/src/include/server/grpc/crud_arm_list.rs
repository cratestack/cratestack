//! [`build_list_arm`] — split out of `crud_arms.rs` (which holds the
//! other four CRUD arm builders) to keep both files under this repo's
//! 200-LoC convention. Its only real distinctness from the other four is
//! the paged-vs-unpaged branch below; see `crud_arms.rs`'s module doc for
//! the shape all five arms share.

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

use super::arm_support::status_from_bridge_error;
use super::crud_arm_spec::{ArmSpec, build_unary_arm, method_path};

pub(super) fn build_list_arm(package: &str, model: &Model) -> proc_macro2::TokenStream {
    let path = method_path(package, &model.name, "List");
    let dispatch_ident = ident(&format!(
        "handle_list_{}_dispatch",
        pluralize(&to_snake_case(&model.name))
    ));
    let request_ty = ident(&format!("{}RpcListInput", model.name));
    let response_ty = ident(&format!("PageOf{}", model.name));
    let svc_ident = ident(&format!("Grpc{}ListSvc", model.name));
    let model_ident = ident(&model.name);
    let status = status_from_bridge_error(quote! { code }, quote! { message });
    // The wire contract always wraps `list` in `PageOf<Model>` (§4.6's
    // gRPC-specific rule — `cratestack-proto::emit::synth_page`'s module
    // doc), but the *dispatch fn we delegate to* is unchanged from
    // REST/RPC, and its response shape still depends on the model's own
    // `@@paged` attribute: paged models genuinely return `Page<Model>`,
    // unpaged ones return a bare `Vec<Model>`. Getting this wrong is a
    // silent codec decode failure (`CoolError::Codec`), not a type error —
    // there is no compile-time signal, only a wrong assumption about what
    // bytes the codec decodes. Branch on it explicitly rather than
    // guessing one shape.
    let bridge_and_wrap = if crate::shared::is_paged_model(model) {
        quote! {
            let page: ::cratestack::Page<super::#model_ident> = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                Ok(value) => value,
                Err((code, message)) => return Err(#status),
            };
            pb::#response_ty::from(&page)
        }
    } else {
        quote! {
            let items: Vec<super::#model_ident> = match ::cratestack::__private::bridge_grpc_response(response, &state.codec, &headers).await {
                Ok(value) => value,
                Err((code, message)) => return Err(#status),
            };
            let page = ::cratestack::Page::new(
                items,
                ::cratestack::PageInfo {
                    limit: None,
                    offset: None,
                    has_next_page: false,
                    has_previous_page: false,
                },
            );
            pb::#response_ty::from(&page)
        }
    };
    let body = quote! {
        let domain_query = message.into_domain();
        let raw_query = ::cratestack::rpc::synthesize_list_query(&domain_query);
        let response = super::axum::#dispatch_ident(state.clone(), canonical, headers.clone(), raw_query).await;
        let wire_value = { #bridge_and_wrap };
        Ok(::cratestack::grpc::tonic::Response::new(wire_value))
    };
    build_unary_arm(ArmSpec {
        path,
        request_ty,
        response_ty,
        svc_ident,
        body,
    })
}
