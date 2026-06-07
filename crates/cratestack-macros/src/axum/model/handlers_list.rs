//! `GET /<plural>` list handler tokens — parses + validates the query
//! string, builds the list request, optionally fetches a total count
//! (paged models), serializes each record through the projection
//! helper, and emits a paged/non-paged response shape accordingly.

use quote::quote;

use super::prep::ModelHandlerPrep;

pub(super) fn build_list_handler(p: &ModelHandlerPrep) -> proc_macro2::TokenStream {
    let list_handler_ident = &p.list_handler_ident;
    let list_dispatch_ident = &p.list_dispatch_ident;
    let list_route_path = &p.list_route_path;
    let model_name = &p.model_name;
    let paged = p.paged;
    let list_capabilities = &p.list_capabilities;
    let list_header_error_encoder = &p.list_header_error_encoder;
    let list_response_type = &p.list_response_type;
    let validate_selection_ident = &p.validate_selection_ident;
    let list_builder_ident = &p.list_builder_ident;
    let serialize_model_value_ident = &p.serialize_model_value_ident;
    let accessor_ident = &p.accessor_ident;
    let total_count_block = &p.total_count_block;
    let list_success_value = &p.list_success_value;
    let list_result_log = &p.list_result_log;

    quote! {
        // REST mount (`GET /<plural>`): canonical request identity is the REST route path.
        async fn #list_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
            RawQuery(raw_query): RawQuery,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let canonical_query = raw_query.clone();
            #list_dispatch_ident(
                state,
                CanonicalRequest {
                    method: "GET",
                    path: #list_route_path,
                    query: canonical_query.as_deref(),
                    body: &[],
                },
                headers,
                raw_query,
            ).await
        }

        // Shared body. `canonical` carries the request's canonical identity
        // (method/path/query/body) used for BOTH signature verification
        // (`request_context`) and the `cratestack_route` tracing field. REST
        // passes `GET /<plural>` with an empty body; RPC dispatch passes
        // `POST /rpc/model.<M>.list` with the raw frame bytes.
        async fn #list_dispatch_ident<C, Auth>(
            state: ModelRouterState<C, Auth>,
            canonical: CanonicalRequest<'_>,
            headers: HeaderMap,
            raw_query: Option<String>,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #list_capabilities;
            let canonical_route = canonical.path;
            let span = ::cratestack::tracing::info_span!(
                "cratestack_model_list_route",
                cratestack_route = canonical_route,
                cratestack_model = #model_name,
                cratestack_operation = "list",
                cratestack_paged = #paged,
            );
            let _span_guard = span.enter();
            let started = ::std::time::Instant::now();

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    "cratestack model list preflight failed",
                );
                return #list_header_error_encoder;
            }
            let request = request_context(canonical.method, canonical.path, canonical.query, &headers, canonical.body);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    let error: CoolError = error.into();
                    ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_route = canonical_route,
                        cratestack_model = #model_name,
                        cratestack_operation = "list",
                        cratestack_error = error.code(),
                        cratestack_detail = error.detail().unwrap_or(""),
                        "cratestack model list auth failed",
                    );
                    return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            let query = match parse_model_list_query(raw_query.as_deref()) {
                Ok(query) => query,
                Err(error) => {
                    ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_route = canonical_route,
                        cratestack_model = #model_name,
                        cratestack_operation = "list",
                        cratestack_error = error.code(),
                        cratestack_detail = error.detail().unwrap_or(""),
                        "cratestack model list query parsing failed",
                    );
                    return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };
            if query.limit.is_some_and(|limit| limit < 0) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_model = #model_name, cratestack_operation = "list", "cratestack model list rejected negative limit");
                return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(CoolError::BadRequest("limit must be greater than or equal to 0".to_owned())));
            }
            if query.offset.is_some_and(|offset| offset < 0) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_model = #model_name, cratestack_operation = "list", "cratestack model list rejected negative offset");
                return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(CoolError::BadRequest("offset must be greater than or equal to 0".to_owned())));
            }
            if let Err(error) = #validate_selection_ident(&query.selection, state.db.#accessor_ident().descriptor()) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_model = #model_name, cratestack_operation = "list", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack model list selection validation failed");
                return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }

            let request = match #list_builder_ident(&state.db, &query, true) {
                Ok(request) => request,
                Err(error) => {
                    return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                }
            };

            #total_count_block

            let result = match request.run(&ctx).await {
                Ok(records) => {
                    let mut values = Vec::with_capacity(records.len());
                    let mut error = None;
                    for record in &records {
                        match #serialize_model_value_ident(&state.db, &ctx, record, &query.selection).await {
                            Ok(value) => values.push(value),
                            Err(inner) => {
                                error = Some(inner);
                                break;
                            }
                        }
                    }
                    match error {
                        Some(error) => Err(error),
                        None => #list_success_value,
                    }
                }
                Err(error) => Err(error),
            };
            #list_result_log
            ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result)
        }
    }
}
