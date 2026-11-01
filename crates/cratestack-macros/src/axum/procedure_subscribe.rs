//! REST's route for a `subscription procedure`, `GET <procedure path>`
//! (`/$procs/<name>`, `@api_version` applied; slice S4): the arguments as
//! JSON-valued query parameters, the stream as SSE. Same shape as the RPC
//! handler (`include/server/rpc_module/subscribe.rs`): check `Accept`,
//! authenticate the canonical request, open through the schema's
//! subscription module, encode.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::{ident, to_snake_case};

/// The handler and its `.route(...)`.
pub(crate) fn generate_subscription_procedure_axum(
    procedure: &Procedure,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let snake = to_snake_case(&procedure.name);
    let handler = ident(&format!("handle_subscribe_{snake}"));
    let open = ident(&format!("open_procedure_{snake}"));
    let path = cratestack_core::procedure_route::procedure_rest_route_path(procedure);
    let handler_def = quote! {
        async fn #handler<R, CR, C, Auth>(
            State(state): State<ProcedureRouterState<R, CR, C, Auth>>,
            headers: HeaderMap,
            RawQuery(raw_query): RawQuery,
            client_ip_ctx: ClientIpContext,
        ) -> Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: AuthProvider,
        {
            if let Err(error) = ::cratestack::__private::validate_subscribe_accept_header(&headers) {
                return ::cratestack::__private::encode_subscribe_error(&error);
            }
            let request = request_context("GET", #path, raw_query.as_deref(), &headers, &[], &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_envelope(::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer), &client_ip_ctx.extensions),
                Err(error) => {
                    let error: CratestackError = error.into();
                    return ::cratestack::__private::encode_subscribe_error(&error);
                }
            };
            match super::subscriptions::#open(&state.db, &state.registry, raw_query, ctx).await {
                Ok(stream) => ::cratestack::__private::encode_subscription_sse_response(stream),
                Err(error) => ::cratestack::__private::encode_subscribe_error(&error),
            }
        }
    };
    let route = quote! { .route(#path, axum::routing::get(#handler)) };
    (handler_def, route)
}
