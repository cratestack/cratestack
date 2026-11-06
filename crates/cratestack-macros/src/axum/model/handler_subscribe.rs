//! REST's `GET /<plural>/$subscribe` (`docs/design/subscriptions-websocket.md`
//! §7.2, slice S3): the SSE binding on a REST schema. Same shape as the RPC
//! handler (`include/server/rpc_module/subscribe.rs`): check `Accept`, authenticate the
//! canonical request, open through the schema's dispatcher with the query
//! string as the feed's input, encode. Errors before the stream are JSON with
//! their own status.

use cratestack_core::{Model, ModelVerb};
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

pub(super) fn build_subscribe_handler(model: &Model) -> proc_macro2::TokenStream {
    if !cratestack_core::model_verbs(model).contains(&ModelVerb::Subscribe) {
        return proc_macro2::TokenStream::new();
    }
    let handler = subscribe_handler_ident(model);
    let route = cratestack_core::model_subscribe_route(&model.name);
    let open = ident(&format!("open_{}", to_snake_case(&model.name)));
    quote! {
        async fn #handler<CR, C, Auth>(
            State(state): State<ModelRouterState<CR, C, Auth>>,
            headers: HeaderMap,
            RawQuery(raw_query): RawQuery,
            client_ip_ctx: ClientIpContext,
        ) -> Response
        where
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            if let Err(error) = ::cratestack::__private::validate_subscribe_accept_header(&headers) {
                return ::cratestack::__private::encode_subscribe_error(&error);
            }
            let request = request_context("GET", #route, raw_query.as_deref(), &headers, &[], &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_envelope(::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer), &client_ip_ctx.extensions),
                Err(error) => {
                    let error: CratestackError = error.into();
                    return ::cratestack::__private::encode_subscribe_error(&error);
                }
            };
            // The model's own feed, not the dispatcher: this router holds no
            // `ProcedureRegistry` for a schema's subscription procedures.
            match super::subscriptions::#open(&state.db, raw_query, ctx).await {
                Ok(stream) => ::cratestack::__private::encode_subscription_sse_response(stream),
                Err(error) => ::cratestack::__private::encode_subscribe_error(&error),
            }
        }
    }
}

pub(crate) fn subscribe_handler_ident(model: &Model) -> syn::Ident {
    ident(&format!(
        "handle_subscribe_{}",
        pluralize(&to_snake_case(&model.name))
    ))
}
