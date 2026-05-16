//! Create/get/update/delete handler tokens. Each delegates to the
//! per-model `Cratestack` accessor after transport / auth / decode preflight.

use quote::quote;

use super::prep::ModelHandlerPrep;

pub(super) fn build_create_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let create_handler_ident = &p.create_handler_ident;
    let write_capabilities = &p.write_capabilities;
    let model_ident = &p.model_ident;
    let list_route_path = &p.list_route_path;
    let create_input_ident = &p.create_input_ident;
    let accessor_ident = &p.accessor_ident;
    let create_auth_preflight = &p.create_auth_preflight;

    quote! {
        async fn #create_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
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
            let request = request_context("POST", #list_route_path, None, &headers, body.as_ref());
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
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

            ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::CREATED, result)
        }
    }
}

pub(super) fn build_get_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let get_handler_ident = &p.get_handler_ident;
    let detail_capabilities = &p.detail_capabilities;
    let primary_key_type = &p.primary_key_type;
    let list_route_path = &p.list_route_path;
    let model_name = &p.model_name;
    let accessor_ident = &p.accessor_ident;
    let validate_selection_ident = &p.validate_selection_ident;
    let serialize_model_value_ident = &p.serialize_model_value_ident;
    let get_etag_extract_decl = &p.get_etag_extract_decl;
    let get_etag_capture = &p.get_etag_capture;
    let get_etag_apply = &p.get_etag_apply;

    quote! {
        async fn #get_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
            Path(id): Path<#primary_key_type>,
            RawQuery(raw_query): RawQuery,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #detail_capabilities;

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::serde_json::Value>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request_path = format!("{}/{}", #list_route_path, id);
            let request = request_context("GET", &request_path, raw_query.as_deref(), &headers, &[]);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
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
            #get_etag_extract_decl
            let result = match state.db.#accessor_ident().find_unique(id).run(&ctx).await {
                Ok(Some(record)) => {
                    #get_etag_capture
                    #serialize_model_value_ident(&state.db, &ctx, &record, &query.selection).await
                }
                Ok(None) => Err(CoolError::NotFound(format!("{} not found", #model_name))),
                Err(error) => Err(error),
            };

            let mut response = ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result);
            #get_etag_apply
            response
        }
    }
}

pub(super) fn build_delete_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let delete_handler_ident = &p.delete_handler_ident;
    let detail_capabilities = &p.detail_capabilities;
    let primary_key_type = &p.primary_key_type;
    let model_ident = &p.model_ident;
    let list_route_path = &p.list_route_path;
    let accessor_ident = &p.accessor_ident;

    quote! {
        async fn #delete_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
            Path(id): Path<#primary_key_type>,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #detail_capabilities;

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request_path = format!("{}/{}", #list_route_path, id);
            let request = request_context("DELETE", &request_path, None, &headers, &[]);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error.into()));
                }
            };

            let result = state.db.#accessor_ident().delete(id).run(&ctx).await;

            ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result)
        }
    }
}
