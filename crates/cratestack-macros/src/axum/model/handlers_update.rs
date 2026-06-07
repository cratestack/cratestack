//! `PATCH /<plural>/{id}` update handler tokens. The largest of the
//! CRUD handlers because it pulls in the `@version` ETag flow (when
//! the model declares one) on both ends of the call.

use quote::quote;

use super::prep::ModelHandlerPrep;

pub(super) fn build_update_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let update_handler_ident = &p.update_handler_ident;
    let update_dispatch_ident = &p.update_dispatch_ident;
    let write_capabilities = &p.write_capabilities;
    let primary_key_type = &p.primary_key_type;
    let model_ident = &p.model_ident;
    let list_route_path = &p.list_route_path;
    let update_input_ident = &p.update_input_ident;
    let accessor_ident = &p.accessor_ident;
    let update_empty_patch_preflight = &p.update_empty_patch_preflight;
    let update_if_match_decl = &p.update_if_match_decl;
    let update_if_match_apply = &p.update_if_match_apply;
    let update_etag_extract = &p.update_etag_extract;
    let update_etag_apply = &p.update_etag_apply;

    quote! {
        // REST mount (`PATCH /<plural>/{id}`): canonical request identity is the REST
        // route path `/<plural>/<id>`.
        async fn #update_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
            Path(id): Path<#primary_key_type>,
            body: Bytes,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let request_path = format!("{}/{}", #list_route_path, id);
            #update_dispatch_ident(state, &request_path, headers, id, body).await
        }

        // Shared body. `canonical_route` is the canonical identity for signature
        // verification and tracing. REST passes `/<plural>/<id>`; RPC dispatch passes
        // the op id (`model.<M>.update`). `id` is still used for the update.
        async fn #update_dispatch_ident<C, Auth>(
            state: ModelRouterState<C, Auth>,
            canonical_route: &str,
            headers: HeaderMap,
            id: #primary_key_type,
            body: Bytes,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #write_capabilities;

            if let Err(error) = ::cratestack::validate_transport_request_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request = request_context("PATCH", canonical_route, None, &headers, body.as_ref());
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error.into()));
                }
            };
            let input = match ::cratestack::decode_transport_request_for::<_, super::inputs::#update_input_ident>(&state.codec, &headers, &CAPABILITIES, &body) {
                Ok(input) => input,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            #update_empty_patch_preflight

            #update_if_match_decl

            let result = state.db.#accessor_ident().update(id).set(input)#update_if_match_apply.run(&ctx).await;

            #update_etag_extract
            let mut response = ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result);
            #update_etag_apply
            response
        }
    }
}
