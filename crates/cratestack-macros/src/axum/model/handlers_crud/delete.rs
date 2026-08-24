//! `DELETE /<plural>/{id}` delete handler tokens.

use quote::quote;

use super::super::prep::ModelHandlerPrep;
use super::response_tail::build_projected_response_tail;

pub(in super::super) fn build_delete_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let delete_handler_ident = &p.delete_handler_ident;
    let delete_dispatch_ident = &p.delete_dispatch_ident;
    let detail_capabilities = &p.detail_capabilities;
    let primary_key_type = &p.primary_key_type;
    let model_ident = &p.model_ident;
    let list_route_path = &p.list_route_path;
    let accessor_ident = &p.accessor_ident;
    let delete_if_match_decl = &p.delete_if_match_decl;
    let delete_if_match_apply = &p.delete_if_match_apply;
    let delete_response_tail =
        build_projected_response_tail(p, quote! { axum::http::StatusCode::OK });

    quote! {
        // REST mount (`DELETE /<plural>/{id}`): canonical request identity is the REST
        // route path `/<plural>/<id>`.
        async fn #delete_handler_ident<CR, C, Auth>(
            State(state): State<ModelRouterState<CR, C, Auth>>,
            headers: HeaderMap,
            Path(id): Path<#primary_key_type>,
            client_ip_ctx: ClientIpContext,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let request_path = format!("{}/{}", #list_route_path, id);
            #delete_dispatch_ident(
                state,
                CanonicalRequest {
                    method: "DELETE",
                    path: &request_path,
                    query: None,
                    body: &[],
                },
                headers,
                client_ip_ctx,
                id,
            ).await
        }

        // Shared body. `canonical` carries the canonical identity (method/path/
        // query/body) for signature verification and tracing. REST passes
        // `DELETE /<plural>/<id>` with an empty body; RPC dispatch passes
        // `POST /rpc/model.<M>.delete` with the raw `{id}` frame bytes (so the
        // id is bound to the signature). `id` is still used for `delete`.
        pub(super) async fn #delete_dispatch_ident<CR, C, Auth>(
            state: ModelRouterState<CR, C, Auth>,
            canonical: CanonicalRequest<'_>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            id: #primary_key_type,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #detail_capabilities;

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request = request_context(canonical.method, canonical.path, canonical.query, &headers, canonical.body, &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer),
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error.into()));
                }
            };

            #delete_if_match_decl

            let result = state.db.#accessor_ident().delete(id)#delete_if_match_apply.run(&ctx).await;

            #delete_response_tail
        }
    }
}
