//! `model.<X>.subscribe` RPC dispatch arm — the SSE subscription
//! endpoint body (`GET /rpc/subscribe/{op_id}`, design doc §3.4a,
//! cratestack#390). Only emitted for models carrying `@@subscribe`.
//!
//! Auth is header-based, mirroring every other HTTP RPC binding (§3.4a:
//! "no upgrade-time HMAC — SSE has no upgrade handshake to sign").
//!
//! The arm is a binding adapter (`docs/design/subscriptions-websocket.md`
//! §6.1, slice S5a): it checks `Accept`, authenticates, and opens the
//! subscription through the schema's generated `SubscriptionDispatcher`
//! ([`super::subscription_module`]), which owns the policy and the
//! per-event read check. A refusal at open is an ordinary RPC error
//! response; once open, the stream is encoded by
//! `encode_subscription_sse_response`, on the same wire as before.

use cratestack_core::Model;
use quote::quote;

pub(crate) fn generate_model_subscribe_dispatch_arm(
    model: &Model,
) -> Option<proc_macro2::TokenStream> {
    if !cratestack_core::model_verbs(model).contains(&cratestack_core::ModelVerb::Subscribe) {
        return None;
    }
    let op_id = cratestack_core::ModelVerb::Subscribe.rpc_op_id(&model.name);
    let canonical_path = format!("/rpc/subscribe/{op_id}");

    Some(quote! {
        #op_id => {
            if let Err(error) = ::cratestack::__private::validate_subscribe_accept_header(&headers) {
                return rpc_dispatch_error(&state, &headers, error);
            }
            let request = request_context("GET", #canonical_path, None, &headers, &[], &client_ip_ctx.extensions);
            let ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_envelope(::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer), &client_ip_ctx.extensions),
                Err(error) => {
                    let error: ::cratestack::CratestackError = error.into();
                    return rpc_dispatch_error(&state, &headers, error);
                }
            };
            let open = ::cratestack::SubscriptionOpen {
                op_id: #op_id.to_owned(),
                input: ::cratestack::serde_json::Value::Null,
                ctx,
            };
            match ::cratestack::SubscriptionDispatcher::open(&state.db.subscriptions(), open).await {
                Ok(stream) => ::cratestack::__private::encode_subscription_sse_response(stream),
                Err(error) => rpc_dispatch_error(&state, &headers, error),
            }
        }
    })
}
