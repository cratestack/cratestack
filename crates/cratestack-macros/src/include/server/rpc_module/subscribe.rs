//! `/rpc/subscribe/{op_id}` handler tokens: SSE subscription dispatch
//! (design doc §3.4a, cratestack#390; `subscriptions-websocket.md` §7.1).
//!
//! `GET` takes the subscription's input as the query string (a model feed's
//! `list` filters, a subscription procedure's JSON-valued arguments), so an
//! `EventSource` can open it. `POST` takes it as a JSON body, the input a
//! WebTransport request carries. Both check `Accept`, authenticate the
//! canonical request, open through the schema's dispatcher and encode the
//! stream; a refusal is a JSON body with its own status.

use quote::quote;

/// `ops`: the schema's subscription op ids. `dispatcher`: the expression,
/// over `state`, that builds the schema's `SubscriptionDispatch`.
pub(super) fn build_subscribe_block(
    ops: &[proc_macro2::TokenStream],
    dispatcher: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if ops.is_empty() {
        return no_subscriptions_block();
    }
    let authenticate = authenticate_tokens();
    quote! {
        const RPC_SUBSCRIPTION_OPS: &[&str] = &[#(#ops),*];

        fn rpc_unknown_subscription(op_id: &str) -> ::cratestack::axum::response::Response {
            ::cratestack::tracing::warn!(
                target: "cratestack",
                cratestack_operation = "rpc_subscribe_dispatch",
                cratestack_op_id = op_id,
                "unknown subscription op id",
            );
            ::cratestack::__private::encode_subscribe_error(&::cratestack::CratestackError::NotFound(
                format!("unknown subscription op `{op_id}`"),
            ))
        }

        async fn rpc_subscribe_dispatch<R, CR, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, CR, C, Auth>>,
            ::cratestack::axum::extract::Path(mut op_params):
                ::cratestack::axum::extract::Path<Vec<String>>,
            ::cratestack::axum::extract::RawQuery(raw_query):
                ::cratestack::axum::extract::RawQuery,
            headers: ::cratestack::axum::http::HeaderMap,
            client_ip_ctx: ClientIpContext,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let op_id = op_params.pop().unwrap_or_default();
            if !RPC_SUBSCRIPTION_OPS.contains(&op_id.as_str()) {
                return rpc_unknown_subscription(&op_id);
            }
            let body: &[u8] = &[];
            let method = "GET";
            #authenticate
            match #dispatcher.open_query(&op_id, raw_query, ctx).await {
                Ok(stream) => ::cratestack::__private::encode_subscription_sse_response(stream),
                Err(error) => ::cratestack::__private::encode_subscribe_error(&error),
            }
        }

        async fn rpc_subscribe_post_dispatch<R, CR, C, Auth>(
            ::cratestack::axum::extract::State(state):
                ::cratestack::axum::extract::State<RpcRouterState<R, CR, C, Auth>>,
            ::cratestack::axum::extract::Path(mut op_params):
                ::cratestack::axum::extract::Path<Vec<String>>,
            ::cratestack::axum::extract::RawQuery(raw_query):
                ::cratestack::axum::extract::RawQuery,
            headers: ::cratestack::axum::http::HeaderMap,
            client_ip_ctx: ClientIpContext,
            body: ::cratestack::axum::body::Bytes,
        ) -> ::cratestack::axum::response::Response
        where
            R: super::procedures::ProcedureRegistry,
            CR: super::computed::ComputedFieldResolver,
            C: HttpTransport,
            Auth: ::cratestack::AuthProvider,
        {
            let op_id = op_params.pop().unwrap_or_default();
            if !RPC_SUBSCRIPTION_OPS.contains(&op_id.as_str()) {
                return rpc_unknown_subscription(&op_id);
            }
            let input = if body.is_empty() {
                ::cratestack::serde_json::Value::Null
            } else {
                match ::cratestack::serde_json::from_slice(&body) {
                    Ok(input) => input,
                    Err(error) => {
                        return ::cratestack::__private::encode_subscribe_error(
                            &::cratestack::CratestackError::BadRequest(format!(
                                "a subscription's body is its JSON input: {error}",
                            )),
                        );
                    }
                }
            };
            let body: &[u8] = &body;
            let method = "POST";
            #authenticate
            let open = ::cratestack::SubscriptionOpen { op_id, input, ctx };
            match ::cratestack::SubscriptionDispatcher::open(&#dispatcher, open).await {
                Ok(stream) => ::cratestack::__private::encode_subscription_sse_response(stream),
                Err(error) => ::cratestack::__private::encode_subscribe_error(&error),
            }
        }
    }
}

/// A schema with no subscription op: both handlers answer `not_found`.
fn no_subscriptions_block() -> proc_macro2::TokenStream {
    quote! {
        async fn rpc_subscribe_dispatch(
            ::cratestack::axum::extract::Path(mut op_params):
                ::cratestack::axum::extract::Path<Vec<String>>,
        ) -> ::cratestack::axum::response::Response {
            let op_id = op_params.pop().unwrap_or_default();
            ::cratestack::tracing::warn!(
                target: "cratestack",
                cratestack_operation = "rpc_subscribe_dispatch",
                cratestack_op_id = op_id.as_str(),
                "unknown subscription op id",
            );
            ::cratestack::__private::encode_subscribe_error(&::cratestack::CratestackError::NotFound(
                format!("unknown subscription op `{op_id}`"),
            ))
        }

        use rpc_subscribe_dispatch as rpc_subscribe_post_dispatch;
    }
}

/// Checks `Accept`, then authenticates the canonical request into `ctx`.
fn authenticate_tokens() -> proc_macro2::TokenStream {
    quote! {
        if let Err(error) = ::cratestack::__private::validate_subscribe_accept_header(&headers) {
            return ::cratestack::__private::encode_subscribe_error(&error);
        }
        let canonical_path = format!("/rpc/subscribe/{op_id}");
        let request = request_context(method, &canonical_path, raw_query.as_deref(), &headers, body, &client_ip_ctx.extensions);
        let ctx = match state.auth_provider.authenticate(&request).await {
            Ok(ctx) => ::cratestack::enrich_context_from_envelope(::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer), &client_ip_ctx.extensions),
            Err(error) => {
                let error: ::cratestack::CratestackError = error.into();
                return ::cratestack::__private::encode_subscribe_error(&error);
            }
        };
    }
}
