use cratestack_core::{Field, Model, Procedure, TypeArity};
use quote::quote;

use crate::relation::{
    generate_relation_include_arm, generate_relation_include_fields_validation_arm,
    generate_relation_include_path_validation_arm, generate_relation_order_by_arms,
    generate_relation_query_guard,
};
use crate::shared::{
    ident, is_paged_model, is_primary_key, model_name_set, pluralize,
    query_scalar_list_parser_tokens, query_scalar_parser_tokens, relation_model_fields,
    rust_type_tokens, scalar_model_fields, to_snake_case,
};
use crate::transport::{
    model_read_transport_capabilities_tokens, model_write_transport_capabilities_tokens,
    procedure_transport_capabilities_tokens,
};

pub(crate) fn generate_procedure_axum_handler(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let procedure_name = &procedure.name;
    let route_path = procedure_route_path(procedure);
    let deprecation_header = procedure_deprecation_header_tokens(procedure);
    let procedure_capabilities = procedure_transport_capabilities_tokens(procedure);
    let result_encoder = if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! { ::cratestack::encode_transport_sequence_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result) }
    } else {
        quote! { ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, result) }
    };

    Ok(quote! {
        async fn #handler_ident<R, C, Auth>(
            State(state): State<ProcedureRouterState<R, C, Auth>>,
            headers: HeaderMap,
            body: Bytes,
        ) -> Response
        where
            R: super::procedures::ProcedureRegistry,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #procedure_capabilities;
            let span = ::cratestack::tracing::info_span!(
                "cratestack_procedure_route",
                cratestack_route = #route_path,
                cratestack_procedure = #procedure_name,
                cratestack_operation = "procedure",
            );
            let _span_guard = span.enter();
            let started = ::std::time::Instant::now();

            if let Err(error) = ::cratestack::validate_transport_request_headers_for(&state.codec, &headers, &CAPABILITIES) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #route_path, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure preflight failed");
                let result: Result<super::procedures::#module_ident::Output, ::cratestack::CoolError> = Err(error);
                return #result_encoder;
            }
            let request = request_context("POST", #route_path, None, &headers, body.as_ref());
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    let error: ::cratestack::CoolError = error.into();
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #route_path, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure auth failed");
                    let result: Result<super::procedures::#module_ident::Output, ::cratestack::CoolError> = Err(error);
                    return #result_encoder;
                }
            };
            let args = match ::cratestack::decode_transport_request_for::<_, super::procedures::#module_ident::Args>(&state.codec, &headers, &CAPABILITIES, &body) {
                Ok(args) => args,
                Err(error) => {
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #route_path, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure decode failed");
                    let result: Result<super::procedures::#module_ident::Output, ::cratestack::CoolError> = Err(error);
                    return #result_encoder;
                }
            };
            let registry = state.registry.clone();
            let db = state.db.clone();
            let auth_db = db.clone();
            let call_args = args.clone();
            let call_ctx = ctx.clone();
            let result = super::procedures::#module_ident::invoke_with_db(&auth_db, &args, &ctx, || async move {
                registry.#method_ident(&db, &call_ctx, call_args).await
            })
            .await;

            match &result {
                Ok(_) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = #route_path,
                    cratestack_procedure = #procedure_name,
                    cratestack_operation = "procedure",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack procedure route completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = #route_path,
                    cratestack_procedure = #procedure_name,
                    cratestack_operation = "procedure",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack procedure route failed",
                ),
            }

            let mut response = #result_encoder;
            #deprecation_header
            response
        }
    })
}

pub(crate) fn generate_procedure_axum_route(procedure: &Procedure) -> proc_macro2::TokenStream {
    let route_path = procedure_route_path(procedure);
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    quote! { .route(#route_path, axum::routing::post(#handler_ident)) }
}

/// Compute the HTTP route path for a procedure, applying any `@api_version`
/// prefix the schema declared. The shape is `/<version>/$procs/<name>` for
/// versioned procedures and `/$procs/<name>` for unversioned ones, so banks
/// can run v1 + v2 of the same procedure side by side.
fn procedure_route_path(procedure: &Procedure) -> String {
    if let Some(version) = procedure_api_version(procedure) {
        format!("/{}/$procs/{}", version, procedure.name)
    } else {
        format!("/$procs/{}", procedure.name)
    }
}

fn procedure_api_version(procedure: &Procedure) -> Option<String> {
    procedure.attributes.iter().find_map(|attribute| {
        attribute
            .raw
            .strip_prefix("@api_version(\"")
            .and_then(|rest| rest.strip_suffix("\")"))
            .map(|s| s.to_owned())
    })
}

/// Token stream that, given a `response` in scope, applies the
/// `Deprecation`/`X-Deprecation` headers when the procedure declared
/// `@deprecated`. Emits empty tokens for non-deprecated procedures.
fn procedure_deprecation_header_tokens(procedure: &Procedure) -> proc_macro2::TokenStream {
    let deprecated = procedure
        .attributes
        .iter()
        .find(|a| a.raw == "@deprecated" || a.raw.starts_with("@deprecated("));
    let Some(attribute) = deprecated else {
        return quote! {};
    };
    let message: Option<String> = attribute
        .raw
        .strip_prefix("@deprecated(\"")
        .and_then(|s| s.strip_suffix("\")"))
        .map(|s| s.to_owned());
    let message_block = match message {
        Some(m) => quote! {
            if let Ok(value) = ::cratestack::axum::http::HeaderValue::from_str(#m) {
                response.headers_mut().insert("X-Deprecation", value);
            }
        },
        None => quote! {},
    };
    quote! {
        response
            .headers_mut()
            .insert("Deprecation", ::cratestack::axum::http::HeaderValue::from_static("true"));
        #message_block
    }
}

pub(crate) fn generate_axum_shared_support() -> proc_macro2::TokenStream {
    quote! {
        fn parse_model_list_query(raw_query: Option<&str>) -> Result<ModelListQuery, CoolError> {
            let mut query = ModelListQuery::default();
            for (key, value) in ::cratestack::parse_query_pairs(raw_query)? {
                match key.as_str() {
                    "limit" => {
                        query.limit = Some(value.parse::<i64>().map_err(|error| {
                            CoolError::BadRequest(format!("invalid value '{}' for limit: {error}", value))
                        })?);
                    }
                    "offset" => {
                        query.offset = Some(value.parse::<i64>().map_err(|error| {
                            CoolError::BadRequest(format!("invalid value '{}' for offset: {error}", value))
                        })?);
                    }
                    "fields" => {
                        query.selection.fields = Some(parse_csv_query_parameter("fields", &value)?);
                    }
                    "include" => {
                        query.selection.includes = parse_csv_query_parameter("include", &value)?;
                    }
                    key if key.starts_with("includeFields[") && key.ends_with(']') => {
                        let include = parse_include_fields_parameter_name(&key)?;
                        let fields = parse_csv_query_parameter(&key, &value)?;
                        if query.selection.include_fields.insert(include.to_owned(), fields).is_some() {
                            return Err(CoolError::BadRequest(format!(
                                "{} must not be provided more than once",
                                key,
                            )));
                        }
                    }
                    "sort" => {
                        query.sort = Some(value);
                    }
                    "orderBy" => {
                        if query.sort.is_some() {
                            return Err(CoolError::BadRequest(
                                "sort and orderBy cannot both be provided".to_owned(),
                            ));
                        }
                        query.sort = Some(value);
                    }
                    "or" => {
                        query.filters.push(::cratestack::QueryExpr::Any(parse_or_group(&value)?));
                    }
                    "where" => {
                        query.filters.push(::cratestack::parse_filter_expression(&value)?);
                    }
                    _ => query.filters.push(::cratestack::QueryExpr::Predicate { key, value }),
                }
            }
            Ok(query)
        }

        fn parse_model_fetch_query(raw_query: Option<&str>) -> Result<ModelFetchQuery, CoolError> {
            let mut query = ModelFetchQuery::default();
            for (key, value) in ::cratestack::parse_query_pairs(raw_query)? {
                match key.as_str() {
                    "fields" => {
                        query.selection.fields = Some(parse_csv_query_parameter("fields", &value)?);
                    }
                    "include" => {
                        query.selection.includes = parse_csv_query_parameter("include", &value)?;
                    }
                    key if key.starts_with("includeFields[") && key.ends_with(']') => {
                        let include = parse_include_fields_parameter_name(&key)?;
                        let fields = parse_csv_query_parameter(&key, &value)?;
                        if query.selection.include_fields.insert(include.to_owned(), fields).is_some() {
                            return Err(CoolError::BadRequest(format!(
                                "{} must not be provided more than once",
                                key,
                            )));
                        }
                    }
                    unexpected => {
                        return Err(CoolError::BadRequest(format!(
                            "unsupported query parameter '{}' for fetch route",
                            unexpected,
                        )));
                    }
                }
            }
            Ok(query)
        }

        fn parse_csv_query_parameter(parameter: &str, value: &str) -> Result<Vec<String>, CoolError> {
            let selections = value
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if selections.is_empty() || selections.iter().any(|selection| selection.is_empty()) {
                return Err(CoolError::BadRequest(format!(
                    "{} must not contain empty selections",
                    parameter,
                )));
            }
            Ok(selections)
        }

        fn parse_include_fields_parameter_name(parameter: &str) -> Result<&str, CoolError> {
            let include = parameter
                .strip_prefix("includeFields[")
                .and_then(|value| value.strip_suffix(']'))
                .ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "invalid includeFields parameter '{}': expected includeFields[relation]",
                        parameter,
                    ))
                })?;
            if include.trim().is_empty() {
                return Err(CoolError::BadRequest(
                    "includeFields[relation] must target a relation name".to_owned(),
                ));
            }
            Ok(include)
        }

        fn parse_or_group(value: &str) -> Result<Vec<::cratestack::QueryExpr>, CoolError> {
            let mut filters = Vec::new();
            for raw_filter in value.split('|') {
                let raw_filter = raw_filter.trim();
                if raw_filter.is_empty() {
                    return Err(CoolError::BadRequest(
                        "or groups must not contain empty filters".to_owned(),
                    ));
                }
                let (key, value) = raw_filter.split_once('=').ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "invalid or filter '{}': expected key=value",
                        raw_filter,
                    ))
                })?;
                filters.push(::cratestack::QueryExpr::Predicate {
                    key: key.to_owned(),
                    value: value.to_owned(),
                });
            }
            if filters.is_empty() {
                return Err(CoolError::BadRequest(
                    "or groups must include at least one filter".to_owned(),
                ));
            }
            Ok(filters)
        }
    }
}

pub(crate) fn generate_model_axum_handlers(
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let version_field_name: Option<String> = model
        .fields
        .iter()
        .find(|field| field.attributes.iter().any(|a| a.raw == "@version"))
        .map(|field| field.name.clone());
    let list_handler_ident = ident(&format!(
        "handle_list_{}",
        pluralize(&to_snake_case(&model.name))
    ));
    let create_handler_ident = ident(&format!(
        "handle_create_{}",
        pluralize(&to_snake_case(&model.name))
    ));
    let get_handler_ident = ident(&format!("handle_get_{}", to_snake_case(&model.name)));
    let update_handler_ident = ident(&format!("handle_update_{}", to_snake_case(&model.name)));
    let delete_handler_ident = ident(&format!("handle_delete_{}", to_snake_case(&model.name)));
    let model_ident = ident(&model.name);
    let field_module_ident = ident(&to_snake_case(&model.name));
    let accessor_ident = ident(&to_snake_case(&model.name));
    let model_name = &model.name;
    let list_route_path = format!("/{}", pluralize(&to_snake_case(&model.name)));
    let create_input_ident = ident(&format!("Create{}Input", model.name));
    let update_input_ident = ident(&format!("Update{}Input", model.name));
    let list_builder_ident = ident(&format!(
        "build_{}_list_request",
        to_snake_case(&model.name)
    ));
    let validate_selection_ident = ident(&format!(
        "validate_{}_selection_query",
        to_snake_case(&model.name)
    ));
    let validate_include_path_ident = ident(&format!(
        "validate_{}_include_path",
        to_snake_case(&model.name)
    ));
    let validate_include_fields_path_ident = ident(&format!(
        "validate_{}_include_fields_path",
        to_snake_case(&model.name)
    ));
    let project_model_value_ident = ident(&format!(
        "project_{}_model_value",
        to_snake_case(&model.name)
    ));
    let project_object_fields_ident = ident(&format!(
        "project_{}_object_fields",
        to_snake_case(&model.name)
    ));
    let project_serialized_value_ident = ident(&format!(
        "project_{}_serialized_value",
        to_snake_case(&model.name)
    ));
    let serialize_model_value_ident = ident(&format!(
        "serialize_{}_model_value",
        to_snake_case(&model.name)
    ));
    let filter_expr_builder_ident =
        ident(&format!("build_{}_filter_expr", to_snake_case(&model.name)));
    let query_expr_builder_ident =
        ident(&format!("build_{}_query_expr", to_snake_case(&model.name)));
    let list_capabilities = model_read_transport_capabilities_tokens();
    let write_capabilities = model_write_transport_capabilities_tokens();
    let detail_capabilities = model_read_transport_capabilities_tokens();
    let paged = is_paged_model(model);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .ok_or_else(|| format!("model {} is missing a primary key", model.name))?;
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let list_response_type = if paged {
        quote! { ::cratestack::Page<::cratestack::serde_json::Value> }
    } else {
        quote! { Vec<::cratestack::serde_json::Value> }
    };
    let list_header_error_encoder = if paged {
        quote! { ::cratestack::encode_transport_result_with_status_for::<_, ::cratestack::Page<::cratestack::serde_json::Value>>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error)) }
    } else {
        quote! { ::cratestack::encode_transport_result_with_status_for::<_, Vec<::cratestack::serde_json::Value>>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error)) }
    };
    let create_auth_preflight = if create_requires_authenticated_context(model) {
        quote! {
            if !ctx.is_authenticated() {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                    &state.codec,
                    &headers,
                    &CAPABILITIES,
                    axum::http::StatusCode::OK,
                    Err(CoolError::Forbidden("create policy denied this operation".to_owned())),
                );
            }
        }
    } else {
        quote! {}
    };
    let update_empty_patch_preflight = quote! {
        if <super::inputs::#update_input_ident as ::cratestack::UpdateModelInput<super::models::#model_ident>>::sql_values(&input).is_empty() {
            return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                &state.codec,
                &headers,
                &CAPABILITIES,
                axum::http::StatusCode::OK,
                Err(CoolError::Validation("update input must contain at least one changed column".to_owned())),
            );
        }
    };
    let (
        update_if_match_decl,
        update_if_match_apply,
        update_etag_extract,
        update_etag_apply,
        get_etag_extract_decl,
        get_etag_capture,
        get_etag_apply,
    ) = match version_field_name.as_deref() {
        Some(name) => {
            let version_field_ident = ident(name);
            (
                quote! {
                    let if_match_version = match ::cratestack::parse_if_match_version(&headers) {
                        Ok(Some(v)) => Some(v),
                        Ok(None) => {
                            return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                                &state.codec,
                                &headers,
                                &CAPABILITIES,
                                axum::http::StatusCode::OK,
                                Err(CoolError::PreconditionFailed("If-Match header required".to_owned())),
                            );
                        }
                        Err(error) => {
                            return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(
                                &state.codec,
                                &headers,
                                &CAPABILITIES,
                                axum::http::StatusCode::OK,
                                Err(error),
                            );
                        }
                    };
                },
                quote! { .if_match(if_match_version.unwrap()) },
                quote! {
                    let etag_version: Option<i64> = match &result {
                        Ok(record) => Some(record.#version_field_ident),
                        Err(_) => None,
                    };
                },
                quote! {
                    if let Some(v) = etag_version {
                        ::cratestack::set_version_etag(&mut response, v);
                    }
                },
                quote! {
                    let mut etag_version: Option<i64> = None;
                },
                quote! {
                    etag_version = Some(record.#version_field_ident);
                },
                quote! {
                    if let Some(v) = etag_version {
                        ::cratestack::set_version_etag(&mut response, v);
                    }
                },
            )
        }
        None => (
            quote! {},
            quote! {},
            quote! {},
            quote! {},
            quote! {},
            quote! {},
            quote! {},
        ),
    };
    let total_count_block = if paged {
        quote! {
            let total_count = {
                let count_request = match #list_builder_ident(&state.db, &query, false) {
                    Ok(request) => request,
                    Err(error) => {
                        return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                    }
                };
                match count_request.run(&ctx).await {
                    Ok(records) => records.len() as i64,
                    Err(error) => {
                        return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
                    }
                }
            };
        }
    } else {
        quote! {}
    };
    let list_success_value = if paged {
        quote! {{
            let limit = query.limit;
            let offset = query.offset.unwrap_or(0);
            Ok(::cratestack::Page::new(
                values,
                ::cratestack::PageInfo {
                    limit,
                    offset: query.offset,
                    has_next_page: limit.is_some_and(|limit| offset + limit < total_count),
                    has_previous_page: offset > 0,
                },
            ).with_total_count(Some(total_count)))
        }}
    } else {
        quote! { Ok(values) }
    };
    let list_result_log = if paged {
        quote! {
            match &result {
                Ok(page) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = true,
                    cratestack_limit = ?query.limit,
                    cratestack_offset = ?query.offset,
                    cratestack_count = page.items.len(),
                    cratestack_total_count = ?page.total_count,
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = true,
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list failed",
                ),
            }
        }
    } else {
        quote! {
            match &result {
                Ok(values) => ::cratestack::tracing::info!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = false,
                    cratestack_limit = ?query.limit,
                    cratestack_offset = ?query.offset,
                    cratestack_count = values.len(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_paged = false,
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack model list failed",
                ),
            }
        }
    };
    let model_names = model_name_set(models);
    let query_filter_arms = scalar_model_fields(model, &model_names)
        .into_iter()
        .filter_map(|field| generate_query_filter_arm(&field_module_ident, field))
        .collect::<Vec<_>>();
    let relation_filter_guards = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_query_guard(model, field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let order_by_arms = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_order_by_arm(&field_module_ident, field))
        .collect::<Vec<_>>();
    let relation_order_by_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_order_by_arms(model, field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let relation_include_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            generate_relation_include_arm(model, field, models, &project_serialized_value_ident)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let relation_include_path_validation_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_include_path_validation_arm(field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let relation_include_fields_validation_arms = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| generate_relation_include_fields_validation_arm(field, model, models))
        .collect::<Result<Vec<_>, String>>()?;

    Ok(quote! {
        fn #filter_expr_builder_ident(
            key: &str,
            value: &str,
        ) -> Result<::cratestack::FilterExpr, CoolError> {
            #(#relation_filter_guards)*
            let (field_name, operator) = key
                .split_once("__")
                .map(|(field_name, operator)| (field_name, operator))
                .unwrap_or((key, "eq"));

            match (field_name, operator) {
                #(#query_filter_arms)*
                _ => Err(CoolError::BadRequest(format!(
                    "unsupported query filter '{}' for {}",
                    key,
                    #model_name,
                ))),
            }
        }

        fn #query_expr_builder_ident(
            expr: &::cratestack::QueryExpr,
        ) -> Result<::cratestack::FilterExpr, CoolError> {
            match expr {
                ::cratestack::QueryExpr::Predicate { key, value } => #filter_expr_builder_ident(key, value),
                ::cratestack::QueryExpr::All(filters) => Ok(::cratestack::FilterExpr::all(
                    filters
                        .iter()
                        .map(#query_expr_builder_ident)
                        .collect::<Result<Vec<_>, CoolError>>()?,
                )),
                ::cratestack::QueryExpr::Any(filters) => Ok(::cratestack::FilterExpr::any(
                    filters
                        .iter()
                        .map(#query_expr_builder_ident)
                        .collect::<Result<Vec<_>, CoolError>>()?,
                )),
                ::cratestack::QueryExpr::Not(filter) => {
                    Ok(#query_expr_builder_ident(filter)?.not())
                }
            }
        }

        fn #validate_selection_ident(
            selection: &ModelSelectionQuery,
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            if let Some(fields) = &selection.fields {
                for field in fields {
                    if !descriptor.allowed_fields.contains(&field.as_str()) {
                        return Err(CoolError::Validation(format!(
                            "unsupported fields selection '{}' for {}",
                            field,
                            #model_name,
                        )));
                    }
                }
            }

            for include in &selection.includes {
                #validate_include_path_ident(include, descriptor)?;
            }

            for (include, fields) in &selection.include_fields {
                if !selection.includes.iter().any(|selected| selected == include) {
                    return Err(CoolError::Validation(format!(
                        "includeFields[{}] requires include={} for {}",
                        include,
                        include,
                        #model_name,
                    )));
                }

                #validate_include_fields_path_ident(include, fields, descriptor)?;
            }

            Ok(())
        }

        fn #validate_include_path_ident(
            include: &str,
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            let (direct, rest) = include
                .split_once('.')
                .map(|(direct, rest)| (direct, Some(rest)))
                .unwrap_or((include, None));
            if !descriptor.allowed_includes.contains(&direct) {
                return Err(CoolError::Validation(format!(
                    "unsupported include selection '{}' for {}",
                    include,
                    #model_name,
                )));
            }

            match (direct, rest) {
                #(#relation_include_path_validation_arms)*
                _ => Ok(()),
            }
        }

        fn #validate_include_fields_path_ident(
            include: &str,
            fields: &[String],
            descriptor: &::cratestack::ModelDescriptor<super::models::#model_ident, #primary_key_type>,
        ) -> Result<(), CoolError> {
            let (direct, rest) = include
                .split_once('.')
                .map(|(direct, rest)| (direct, Some(rest)))
                .unwrap_or((include, None));
            if !descriptor.allowed_includes.contains(&direct) {
                return Err(CoolError::Validation(format!(
                    "unsupported includeFields selection '{}' for {}",
                    include,
                    #model_name,
                )));
            }

            match (direct, rest) {
                #(#relation_include_fields_validation_arms)*
                _ => Ok(()),
            }
        }

        fn #project_object_fields_ident(
            object: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
            fields: &[String],
            context: &str,
        ) -> Result<::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>, CoolError> {
            let mut projected = ::cratestack::serde_json::Map::new();
            for field in fields {
                let value = object.get(field).cloned().ok_or_else(|| {
                    CoolError::Internal(format!(
                        "serialized relation '{}' is missing field '{}'",
                        context,
                        field,
                    ))
                })?;
                projected.insert(field.clone(), value);
            }
            Ok(projected)
        }

        fn #project_serialized_value_ident(
            value: ::cratestack::serde_json::Value,
            fields: Option<&[String]>,
            context: &str,
        ) -> Result<::cratestack::serde_json::Value, CoolError> {
            let Some(fields) = fields else {
                return Ok(value);
            };

            match value {
                ::cratestack::serde_json::Value::Null => Ok(::cratestack::serde_json::Value::Null),
                ::cratestack::serde_json::Value::Object(object) => Ok(::cratestack::serde_json::Value::Object(
                    #project_object_fields_ident(object, fields, context)?,
                )),
                ::cratestack::serde_json::Value::Array(values) => {
                    let mut projected = Vec::with_capacity(values.len());
                    for value in values {
                        projected.push(#project_serialized_value_ident(value, Some(fields), context)?);
                    }
                    Ok(::cratestack::serde_json::Value::Array(projected))
                }
                _ => Err(CoolError::Internal(format!(
                    "included relation '{}' must serialize to an object, array, or null",
                    context,
                ))),
            }
        }

        fn #project_model_value_ident(
            record: &super::models::#model_ident,
            fields: Option<&[String]>,
        ) -> Result<::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>, CoolError> {
            let value = ::cratestack::serde_json::to_value(record)
                .map_err(|error| CoolError::Internal(format!("failed to serialize {}: {error}", #model_name)))?;
            let object = value.as_object().cloned().ok_or_else(|| {
                CoolError::Internal(format!("generated {} serialization must be a JSON object", #model_name))
            })?;

            let Some(fields) = fields else {
                return Ok(object);
            };

            let mut projected = ::cratestack::serde_json::Map::new();
            for field in fields {
                let value = object.get(field).cloned().ok_or_else(|| {
                    CoolError::Internal(format!(
                        "generated {} serialization is missing field '{}'",
                        #model_name,
                        field,
                    ))
                })?;
                projected.insert(field.clone(), value);
            }
            Ok(projected)
        }

        fn #serialize_model_value_ident<'a>(
            db: &'a super::Cratestack,
            ctx: &'a ::cratestack::CoolContext,
            record: &'a super::models::#model_ident,
            selection: &'a ModelSelectionQuery,
        ) -> ::core::pin::Pin<
            Box<dyn ::core::future::Future<Output = Result<::cratestack::serde_json::Value, CoolError>> + Send + 'a>,
        > {
            Box::pin(async move {
                let mut object = #project_model_value_ident(record, selection.fields.as_deref())?;

                for include in selection.direct_includes() {
                    match include.as_str() {
                        #(#relation_include_arms)*
                        _ => unreachable!("validated include should be supported"),
                    }
                }

                Ok(::cratestack::serde_json::Value::Object(object))
            })
        }

        fn #list_builder_ident<'a>(
            db: &'a super::Cratestack,
            query: &ModelListQuery,
            apply_paging: bool,
        ) -> Result<::cratestack::FindMany<'a, super::models::#model_ident, #primary_key_type>, CoolError> {
            let descriptor = db.#accessor_ident().descriptor();
            let mut request = db.#accessor_ident().find_many();

            for filter in &query.filters {
                request = request.where_expr(#query_expr_builder_ident(filter)?);
            }

            if let Some(sort) = &query.sort {
                for raw_term in sort.split(',') {
                    let raw_term = raw_term.trim();
                    if raw_term.is_empty() {
                        return Err(CoolError::BadRequest(
                            "sort must not contain empty fields".to_owned(),
                        ));
                    }

                    let (descending, field_name) = match raw_term.strip_prefix('-') {
                        Some(field_name) => (true, field_name),
                        None => (false, raw_term),
                    };

                    if !descriptor.allowed_sorts.contains(&field_name) {
                        return Err(CoolError::Validation(format!(
                            "unsupported sort field '{}' for {}",
                            field_name,
                            #model_name,
                        )));
                    }

                    request = match field_name {
                        #(#order_by_arms)*
                        #(#relation_order_by_arms)*
                        _ => unreachable!("validated sort should be supported"),
                    };
                }
            }

            if apply_paging {
                if let Some(limit) = query.limit {
                    request = request.limit(limit);
                }
                if let Some(offset) = query.offset {
                    request = request.offset(offset);
                }
            }

            Ok(request)
        }

        async fn #list_handler_ident<C, Auth>(
            State(state): State<ModelRouterState<C, Auth>>,
            headers: HeaderMap,
            RawQuery(raw_query): RawQuery,
        ) -> Response
        where
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #list_capabilities;
            let span = ::cratestack::tracing::info_span!(
                "cratestack_model_list_route",
                cratestack_route = #list_route_path,
                cratestack_model = #model_name,
                cratestack_operation = "list",
                cratestack_paged = #paged,
            );
            let _span_guard = span.enter();
            let started = ::std::time::Instant::now();

            if let Err(error) = ::cratestack::validate_transport_response_headers_for(&state.codec, &headers, &CAPABILITIES) {
                ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = #list_route_path,
                    cratestack_model = #model_name,
                    cratestack_operation = "list",
                    cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""),
                    "cratestack model list preflight failed",
                );
                return #list_header_error_encoder;
            }
            let request = request_context("GET", #list_route_path, raw_query.as_deref(), &headers, &[]);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    let error: CoolError = error.into();
                    ::cratestack::tracing::warn!(
                        target: "cratestack",
                        cratestack_route = #list_route_path,
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
                        cratestack_route = #list_route_path,
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
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #list_route_path, cratestack_model = #model_name, cratestack_operation = "list", "cratestack model list rejected negative limit");
                return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(CoolError::BadRequest("limit must be greater than or equal to 0".to_owned())));
            }
            if query.offset.is_some_and(|offset| offset < 0) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #list_route_path, cratestack_model = #model_name, cratestack_operation = "list", "cratestack model list rejected negative offset");
                return ::cratestack::encode_transport_result_with_status_for::<_, #list_response_type>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(CoolError::BadRequest("offset must be greater than or equal to 0".to_owned())));
            }
            if let Err(error) = #validate_selection_ident(&query.selection, state.db.#accessor_ident().descriptor()) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = #list_route_path, cratestack_model = #model_name, cratestack_operation = "list", cratestack_error = error.code(),
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
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #write_capabilities;

            if let Err(error) = ::cratestack::validate_transport_request_headers_for(&state.codec, &headers, &CAPABILITIES) {
                return ::cratestack::encode_transport_result_with_status_for::<_, super::models::#model_ident>(&state.codec, &headers, &CAPABILITIES, axum::http::StatusCode::OK, Err(error));
            }
            let request_path = format!("{}/{}", #list_route_path, id);
            let request = request_context("PATCH", &request_path, None, &headers, body.as_ref());
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
    })
}

pub(crate) fn generate_model_axum_routes(model: &Model) -> proc_macro2::TokenStream {
    let list_route = format!("/{}", pluralize(&to_snake_case(&model.name)));
    let detail_route = format!("/{}/{{id}}", pluralize(&to_snake_case(&model.name)));
    let list_handler_ident = ident(&format!(
        "handle_list_{}",
        pluralize(&to_snake_case(&model.name))
    ));
    let create_handler_ident = ident(&format!(
        "handle_create_{}",
        pluralize(&to_snake_case(&model.name))
    ));
    let get_handler_ident = ident(&format!("handle_get_{}", to_snake_case(&model.name)));
    let update_handler_ident = ident(&format!("handle_update_{}", to_snake_case(&model.name)));
    let delete_handler_ident = ident(&format!("handle_delete_{}", to_snake_case(&model.name)));

    quote! {
        .route(
            #list_route,
            axum::routing::get(#list_handler_ident).post(#create_handler_ident),
        )
        .route(
            #detail_route,
            axum::routing::get(#get_handler_ident)
                .patch(#update_handler_ident)
                .delete(#delete_handler_ident),
        )
    }
}

fn generate_query_filter_arm(
    field_module_ident: &syn::Ident,
    field: &Field,
) -> Option<proc_macro2::TokenStream> {
    let field_name = &field.name;
    let field_fn = ident(&field.name);
    let scalar_parser = query_scalar_parser_tokens(&field.ty, quote! { value }, field_name)?;
    let mut arms = Vec::new();

    if field.ty.arity == TypeArity::Required {
        let list_parser = query_scalar_list_parser_tokens(&field.ty, field_name)?;
        arms.push(quote! {
            (#field_name, "eq") => {
                let parsed = (#scalar_parser)?;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().eq(parsed)))
            }
        });
        arms.push(quote! {
            (#field_name, "ne") => {
                let parsed = (#scalar_parser)?;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().ne(parsed)))
            }
        });
        arms.push(quote! {
            (#field_name, "in") => {
                let parsed = #list_parser;
                Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().in_(parsed)))
            }
        });
        if crate::shared::supports_comparison(field) {
            arms.push(quote! {
                (#field_name, "lt") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().lt(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "lte") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().lte(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "gt") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().gt(parsed)))
                }
            });
            arms.push(quote! {
                (#field_name, "gte") => {
                    let parsed = (#scalar_parser)?;
                    Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().gte(parsed)))
                }
            });
        }
    }

    if matches!(field.ty.name.as_str(), "String" | "Cuid") {
        match field.ty.arity {
            TypeArity::Required | TypeArity::Optional => {
                arms.push(quote! {
                    (#field_name, "contains") => {
                        Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().contains(value.to_owned())))
                    }
                });
                arms.push(quote! {
                    (#field_name, "startsWith") => {
                        Ok(::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().starts_with(value.to_owned())))
                    }
                });
            }
            TypeArity::List => {}
        }
    }

    if field.ty.arity == TypeArity::Optional {
        arms.push(quote! {
            (#field_name, "isNull") => {
                let parsed = value.parse::<bool>().map_err(|error| {
                    CoolError::BadRequest(format!("invalid value '{}' for {}__isNull: {error}", value, #field_name))
                })?;
                Ok(if parsed {
                    ::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().is_null())
                } else {
                    ::cratestack::FilterExpr::from(super::#field_module_ident::#field_fn().is_not_null())
                })
            }
        });
    }

    if arms.is_empty() {
        None
    } else {
        Some(quote! { #(#arms)* })
    }
}

fn generate_order_by_arm(
    field_module_ident: &syn::Ident,
    field: &Field,
) -> proc_macro2::TokenStream {
    let field_name = &field.name;
    let field_fn = ident(&field.name);

    quote! {
        #field_name => {
            if descending {
                request.order_by(super::#field_module_ident::#field_fn().desc())
            } else {
                request.order_by(super::#field_module_ident::#field_fn().asc())
            }
        }
    }
}

fn create_requires_authenticated_context(model: &Model) -> bool {
    let mut saw_create_allow = false;
    for attribute in &model.attributes {
        let Some((actions, expression)) = parse_model_allow_attribute(&attribute.raw) else {
            continue;
        };
        if !actions
            .iter()
            .any(|action| matches!(action.as_str(), "create" | "all"))
        {
            continue;
        }

        saw_create_allow = true;
        if normalize_policy_expression(&expression) != "auth()!=null" {
            return false;
        }
    }

    saw_create_allow
}

fn parse_model_allow_attribute(raw: &str) -> Option<(Vec<String>, String)> {
    let inner = raw
        .strip_prefix("@@allow(")?
        .strip_suffix(')')?
        .trim()
        .to_owned();
    let mut parts = split_policy_arguments(&inner);
    if parts.len() != 2 {
        return None;
    }
    let expression = parts.pop()?.trim().to_owned();
    let actions = trim_policy_string_literal(&parts.pop()?)?
        .split(',')
        .map(str::trim)
        .map(str::to_owned)
        .filter(|action| !action.is_empty())
        .collect::<Vec<_>>();

    Some((actions, expression))
}

fn split_policy_arguments(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut depth = 0usize;

    for character in value.chars() {
        match (quote, character) {
            (Some(active), candidate) if active == candidate => {
                quote = None;
                current.push(character);
            }
            (Some(_), _) => current.push(character),
            (None, '\'' | '"') => {
                quote = Some(character);
                current.push(character);
            }
            (None, '(') => {
                depth += 1;
                current.push(character);
            }
            (None, ')') => {
                depth = depth.saturating_sub(1);
                current.push(character);
            }
            (None, ',') if depth == 0 => {
                parts.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(character),
        }
    }

    if !current.trim().is_empty() {
        parts.push(current.trim().to_owned());
    }

    parts
}

fn trim_policy_string_literal(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|candidate| candidate.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|candidate| candidate.strip_suffix('\''))
        })
}

fn normalize_policy_expression(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}
