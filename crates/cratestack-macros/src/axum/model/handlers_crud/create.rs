//! `POST /<plural>` create handler tokens.

use quote::quote;

use super::super::prep::ModelHandlerPrep;
use super::response_tail::build_projected_response_tail;

pub(in super::super) fn build_create_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let create_handler_ident = &p.create_handler_ident;
    let create_dispatch_ident = &p.create_dispatch_ident;
    let write_capabilities = &p.write_capabilities;
    let model_ident = &p.model_ident;
    let list_route_path = &p.list_route_path;
    let create_input_ident = &p.create_input_ident;
    let accessor_ident = &p.accessor_ident;
    let create_auth_preflight = &p.create_auth_preflight;
    let create_response_tail =
        build_projected_response_tail(p, quote! { axum::http::StatusCode::CREATED });

    quote! {
        // REST mount (`POST /<plural>`): canonical request identity is the REST route path.
        async fn #create_handler_ident<CR, C, Auth>(
            State(state): State<ModelRouterState<CR, C, Auth>>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: Bytes,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let canonical_body = body.clone();
            #create_dispatch_ident(
                state,
                CanonicalRequest {
                    method: "POST",
                    path: #list_route_path,
                    query: None,
                    body: canonical_body.as_ref(),
                },
                headers,
                client_ip_ctx,
                body,
            ).await
        }

        // Shared body. `canonical` carries the canonical identity (method/path/
        // query/body) for signature verification and tracing. REST passes
        // `POST /<plural>` with the REST body; RPC dispatch passes
        // `POST /rpc/model.<M>.create` with the raw frame bytes.
        pub(super) async fn #create_dispatch_ident<CR, C, Auth>(
            state: ModelRouterState<CR, C, Auth>,
            canonical: CanonicalRequest<'_>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: Bytes,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #write_capabilities;

            if let Err(error) = ::cratestack::validate_transport_request_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request = request_context(canonical.method, canonical.path, canonical.query, &headers, canonical.body, &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer),
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error.into()));
                }
            };
            #create_auth_preflight
            let input = match ::cratestack::decode_transport_request_for::<_, super::inputs::#create_input_ident>(&state.codec, &headers, &CAPABILITIES, &body) {
                Ok(input) => input,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };

            let result = state.db.#accessor_ident().create(input).run(&ctx).await;

            #create_response_tail
        }
    }
}
