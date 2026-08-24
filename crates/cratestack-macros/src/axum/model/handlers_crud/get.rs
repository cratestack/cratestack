//! `GET /<plural>/{id}` fetch handler tokens.

use quote::quote;

use super::super::prep::ModelHandlerPrep;

pub(in super::super) fn build_get_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let get_handler_ident = &p.get_handler_ident;
    let get_dispatch_ident = &p.get_dispatch_ident;
    let detail_capabilities = &p.detail_capabilities;
    let primary_key_type = &p.primary_key_type;
    let list_route_path = &p.list_route_path;
    let model_name = &p.model_name;
    let accessor_ident = &p.accessor_ident;
    let validate_selection_ident = &p.validate_selection_ident;
    let serialize_model_value_ident = &p.serialize_model_value_ident;
    let parse_computed_params_ident = &p.parse_computed_params_ident;
    let get_etag_extract_decl = &p.get_etag_extract_decl;
    let get_etag_capture = &p.get_etag_capture;
    let get_etag_apply = &p.get_etag_apply;

    quote! {
        // REST mount (`GET /<plural>/{id}`): canonical request identity is the REST
        // route path `/<plural>/<id>`.
        async fn #get_handler_ident<CR, C, Auth>(
            State(state): State<ModelRouterState<CR, C, Auth>>,
            headers: HeaderMap,
            Path(id): Path<#primary_key_type>,
            RawQuery(raw_query): RawQuery,
            client_ip_ctx: ClientIpContext,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let request_path = format!("{}/{}", #list_route_path, id);
            let canonical_query = raw_query.clone();
            #get_dispatch_ident(
                state,
                CanonicalRequest {
                    method: "GET",
                    path: &request_path,
                    query: canonical_query.as_deref(),
                    body: &[],
                },
                headers,
                client_ip_ctx,
                id,
                raw_query,
            ).await
        }

        // Shared body. `canonical` carries the canonical identity (method/path/
        // query/body) for signature verification and tracing. REST passes
        // `GET /<plural>/<id>` with an empty body; RPC dispatch passes
        // `POST /rpc/model.<M>.get` with the raw `{id}` frame bytes (so the id
        // is bound to the signature). `id` is still used for `find_unique`.
        pub(super) async fn #get_dispatch_ident<CR, C, Auth>(
            state: ModelRouterState<CR, C, Auth>,
            canonical: CanonicalRequest<'_>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            id: #primary_key_type,
            raw_query: Option<String>,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #detail_capabilities;

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request = request_context(canonical.method, canonical.path, canonical.query, &headers, canonical.body, &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer),
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error.into()));
                }
            };
            let query = match parse_model_fetch_query(raw_query.as_deref()) {
                Ok(query) => query,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            if let Err(error) = #validate_selection_ident(&query.selection, state.db.#accessor_ident().descriptor()) {
                return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            // Validated before any DB access (docs/design/computed-fields.md):
            // a `computedParams` value that isn't a JSON object, or that
            // names an unknown/non-parameterized/`?fields=`-excluded key,
            // never reaches `find_unique`. Decoding a key's *value* into its
            // field's params type is a separate, later step — it happens at
            // resolve time in `serializers::computed_fields`, after the row
            // has already been fetched.
            let computed_params = match #parse_computed_params_ident(query.computed_params.as_deref(), &query.selection) {
                Ok(computed_params) => computed_params,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            #get_etag_extract_decl
            let result = match state.db.#accessor_ident().find_unique(id).run(&ctx).await {
                Ok(Some(record)) => {
                    #get_etag_capture
                    #serialize_model_value_ident(&state.db, &state.resolvers, &ctx, &record, &query.selection, Some(&computed_params)).await
                }
                Ok(None) => Err(CratestackError::NotFound(format!("{} not found", #model_name))),
                Err(error) => Err(error),
            };

            let mut response = ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result);
            #get_etag_apply
            response
        }
    }
}
