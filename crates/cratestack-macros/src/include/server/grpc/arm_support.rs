//! Small pieces shared by both arm-builder families — `crud_arms.rs`'s
//! CRUD arms and `procedure_arms.rs`'s procedure arms — split out of
//! `service.rs` to keep it under this repo's 200-LoC file convention.
//! See `service.rs`'s module doc for the full picture these two pieces
//! fit into.

use quote::quote;

/// Shared prelude every arm needs: auth-relevant headers from gRPC
/// metadata (content-type/accept stripped so codec negotiation falls back
/// to the schema's default codec — see `rpc_inputs.rs`'s module doc and
/// `cratestack_axum::rpc::bridge_grpc_response`'s doc for why that
/// matters), and the canonical request path/body used for both auth and
/// the "known gap" signing note in `service.rs`'s module doc.
pub(super) fn request_prelude(path: &str) -> proc_macro2::TokenStream {
    quote! {
        let mut headers = ::cratestack::grpc::metadata_to_headers(request.metadata());
        // The dispatch fn's own request/response codec negotiation reads
        // `Content-Type`/`Accept`, not gRPC's own (`application/grpc+proto`)
        // — pin both to the schema's CBOR wire codec explicitly (required:
        // `validate_transport_request_headers_for` treats a *missing*
        // `Content-Type` as an error on write verbs, it does not default).
        // This is also what `bridge_grpc_response` decodes the dispatch
        // response against, so both directions agree on one content type.
        headers.remove(::cratestack::grpc::tonic::codegen::http::header::ACCEPT);
        headers.insert(
            ::cratestack::grpc::tonic::codegen::http::header::CONTENT_TYPE,
            ::cratestack::grpc::tonic::codegen::http::HeaderValue::from_static("application/cbor"),
        );
        let message = request.into_inner();
        let canonical_body = ::cratestack::grpc::prost::Message::encode_to_vec(&message);
        let canonical = super::axum::CanonicalRequest {
            method: "POST",
            path: #path,
            query: None,
            body: canonical_body.as_ref(),
        };
    }
}

pub(super) fn status_from_bridge_error(
    code_expr: proc_macro2::TokenStream,
    message_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    quote! {
        ::cratestack::grpc::tonic::Status::new(
            ::cratestack::grpc::cool_error_code_to_tonic_code(&#code_expr),
            #message_expr,
        )
    }
}
