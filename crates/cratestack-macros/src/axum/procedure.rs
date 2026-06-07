//! Per-procedure axum handler + route emission, plus the
//! `@api_version` / `@deprecated` attribute helpers it consumes.

use cratestack_core::{Procedure, TypeArity};
use quote::quote;

use crate::shared::{ident, to_snake_case};
use crate::transport::procedure_transport_capabilities_tokens;

pub(crate) fn generate_procedure_axum_handler(
    procedure: &Procedure,
) -> Result<proc_macro2::TokenStream, String> {
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    let dispatch_ident = ident(&format!("handle_{}_dispatch", to_snake_case(&procedure.name)));
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
        // REST mount (`transport rest` / the `/$procs/<name>` route): the
        // canonical request identity IS the REST route path.
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
            #dispatch_ident(state, #route_path, headers, body).await
        }

        // Shared body. `canonical_route` is the request's canonical identity used
        // for BOTH signature verification (`request_context.path`) and the
        // `cratestack_route` tracing field. REST passes the `/$procs/<name>`
        // route; RPC dispatch passes the op id (`procedure.<name>`) so on
        // `transport rpc` the op id is the single identity for url, dispatch,
        // signing, and logs — `/$procs/*` never appears.
        async fn #dispatch_ident<R, C, Auth>(
            state: ProcedureRouterState<R, C, Auth>,
            canonical_route: &str,
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
                cratestack_route = canonical_route,
                cratestack_procedure = #procedure_name,
                cratestack_operation = "procedure",
            );
            let _span_guard = span.enter();
            let started = ::std::time::Instant::now();

            if let Err(error) = ::cratestack::validate_transport_request_headers_for(&state.codec, &headers, &CAPABILITIES) {
                ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure preflight failed");
                let result: Result<super::procedures::#module_ident::Output, ::cratestack::CoolError> = Err(error);
                return #result_encoder;
            }
            let request = request_context("POST", canonical_route, None, &headers, body.as_ref());
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers),
                Err(error) => {
                    let error: ::cratestack::CoolError = error.into();
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure auth failed");
                    let result: Result<super::procedures::#module_ident::Output, ::cratestack::CoolError> = Err(error);
                    return #result_encoder;
                }
            };
            let args = match ::cratestack::decode_transport_request_for::<_, super::procedures::#module_ident::Args>(&state.codec, &headers, &CAPABILITIES, &body) {
                Ok(args) => args,
                Err(error) => {
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
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
                    cratestack_route = canonical_route,
                    cratestack_procedure = #procedure_name,
                    cratestack_operation = "procedure",
                    cratestack_authenticated = ctx.is_authenticated(),
                    cratestack_duration_ms = started.elapsed().as_millis() as u64,
                    cratestack_request_id = ctx.request_id().unwrap_or(""),
                    "cratestack procedure route completed",
                ),
                Err(error) => ::cratestack::tracing::warn!(
                    target: "cratestack",
                    cratestack_route = canonical_route,
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

/// HTTP route path for a procedure, applying any `@api_version`
/// prefix. Shape is `/<version>/$procs/<name>` for versioned
/// procedures and `/$procs/<name>` otherwise, so banks can run v1 + v2
/// side by side.
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
/// `@deprecated`. Empty tokens for non-deprecated procedures.
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
