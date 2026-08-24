//! Per-procedure axum handler + route emission, plus the
//! `@api_version` / `@deprecated` / `@status` attribute helpers it
//! consumes.

mod dispatch_tail;
mod invoke_call;
mod route_attrs;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use cratestack_core::{Procedure, TypeArity};
use quote::quote;

use crate::shared::{ident, to_snake_case};
use crate::transport::procedure_transport_capabilities_tokens;
use dispatch_tail::procedure_dispatch_tail_tokens;
use invoke_call::procedure_invoke_call_tokens;
use route_attrs::{
    procedure_axum_route_tokens, procedure_deprecation_header_tokens, procedure_route_path,
    procedure_success_status_tokens,
};

pub(crate) fn generate_procedure_axum_handler(
    procedure: &Procedure,
    bearing: &BTreeSet<String>,
) -> Result<proc_macro2::TokenStream, String> {
    let handler_ident = ident(&format!("handle_{}", to_snake_case(&procedure.name)));
    let dispatch_ident = ident(&format!(
        "handle_{}_dispatch",
        to_snake_case(&procedure.name)
    ));
    let method_ident = ident(&to_snake_case(&procedure.name));
    let module_ident = ident(&to_snake_case(&procedure.name));
    let procedure_name = &procedure.name;
    let route_path = procedure_route_path(procedure);
    let deprecation_header = procedure_deprecation_header_tokens(procedure);
    let procedure_capabilities = procedure_transport_capabilities_tokens(procedure);
    let success_status = procedure_success_status_tokens(procedure);
    let result_encoder = if matches!(procedure.return_type.arity, TypeArity::List) {
        quote! { ::cratestack::encode_transport_sequence_result_with_status_for(&state.codec, &headers, &CAPABILITIES, #success_status, result) }
    } else {
        quote! { ::cratestack::encode_transport_result_with_status_for(&state.codec, &headers, &CAPABILITIES, #success_status, result) }
    };
    let invoke_call = procedure_invoke_call_tokens(procedure, &method_ident);
    let dispatch_tail = procedure_dispatch_tail_tokens(
        procedure,
        procedure_name,
        &success_status,
        &result_encoder,
        &deprecation_header,
        bearing,
    );

    Ok(quote! {
        // REST mount (`transport rest` / the `/$procs/<name>` route): the
        // canonical request identity IS the REST route path.
        async fn #handler_ident<R, CR, C, Auth>(
            State(state): State<ProcedureRouterState<R, CR, C, Auth>>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: Bytes,
        ) -> Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let canonical_body = body.clone();
            #dispatch_ident(
                state,
                CanonicalRequest {
                    method: "POST",
                    path: #route_path,
                    query: None,
                    body: canonical_body.as_ref(),
                },
                headers,
                client_ip_ctx,
                body,
            ).await
        }

        // Shared body. `canonical` carries the request's canonical identity
        // (method/path/query/body) used for BOTH signature verification
        // (`request_context`) and the `cratestack_route` tracing field. REST
        // passes the `/$procs/<name>` route with the REST body; RPC dispatch
        // passes `POST /rpc/procedure.<name>` with the raw frame bytes so on
        // `transport rpc` the actual rpc request is the single canonical for
        // url, dispatch, signing, and logs — `/$procs/*` never appears.
        pub(super) async fn #dispatch_ident<R, CR, C, Auth>(
            state: ProcedureRouterState<R, CR, C, Auth>,
            canonical: CanonicalRequest<'_>,
            headers: HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: Bytes,
        ) -> Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            const CAPABILITIES: ::cratestack::RouteTransportCapabilities = #procedure_capabilities;
            let canonical_route = canonical.path;
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
                let result: Result<super::procedures::#module_ident::Output, ::cratestack::CratestackError> = Err(error);
                return #result_encoder;
            }
            let request = request_context(canonical.method, canonical.path, canonical.query, &headers, canonical.body, &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer),
                Err(error) => {
                    let error: ::cratestack::CratestackError = error.into();
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure auth failed");
                    let result: Result<super::procedures::#module_ident::Output, ::cratestack::CratestackError> = Err(error);
                    return #result_encoder;
                }
            };
            let args = match ::cratestack::decode_transport_request_for::<_, super::procedures::#module_ident::Args>(&state.codec, &headers, &CAPABILITIES, &body) {
                Ok(args) => args,
                Err(error) => {
                    ::cratestack::tracing::warn!(target: "cratestack", cratestack_route = canonical_route, cratestack_procedure = #procedure_name, cratestack_operation = "procedure", cratestack_error = error.code(),
                    cratestack_detail = error.detail().unwrap_or(""), "cratestack procedure decode failed");
                    let result: Result<super::procedures::#module_ident::Output, ::cratestack::CratestackError> = Err(error);
                    return #result_encoder;
                }
            };
            let registry = state.registry.clone();
            let db = state.db.clone();
            let auth_db = db.clone();
            let call_args = args.clone();
            let call_ctx = ctx.clone();
            // cratestack#512: `invoke_with_db` hands the closure an
            // `Authorized` witness only it could construct (via the
            // `authorize_with_db` call inside it) — `#invoke_call` threads
            // that witness into the `ProcedureRegistry` method call, which
            // is the only place a value of that type is allowed to end up.
            let result = super::procedures::#module_ident::invoke_with_db(&auth_db, &args, &ctx, |authorized| async move {
                #invoke_call
            })
            .await;

            #dispatch_tail
        }
    })
}

pub(crate) fn generate_procedure_axum_route(procedure: &Procedure) -> proc_macro2::TokenStream {
    procedure_axum_route_tokens(procedure)
}
