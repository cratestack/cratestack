//! `model.<X>.subscribe` RPC dispatch arm — the SSE subscription
//! endpoint body (`GET /rpc/subscribe/{op_id}`, design doc §3.4a,
//! cratestack#390). Only emitted for models carrying `@@subscribe`;
//! `cratestack-parser` guarantees such a model also declares
//! `@@emit(...)`, so [`crate::event::model_emitted_events`] is always
//! non-empty here.
//!
//! Auth is header-based, mirroring every other HTTP RPC binding (§3.4a:
//! "no upgrade-time HMAC — SSE has no upgrade handshake to sign"). Row-
//! level `@@allow` policy is **not** replayed against streamed events —
//! that machinery lives deep in the SQL query builders and has no
//! analogue for an in-memory outbox-sourced event; this is a deliberate,
//! documented scope limit for the first cut (see the PR description),
//! not an oversight.

use cratestack_core::Model;
use quote::quote;

use crate::event::model_emitted_events;
use crate::shared::{ident, to_snake_case};

pub(crate) fn generate_model_subscribe_dispatch_arm(
    model: &Model,
) -> Result<Option<proc_macro2::TokenStream>, String> {
    if !model
        .attributes
        .iter()
        .any(|attribute| attribute.raw == "@@subscribe")
    {
        return Ok(None);
    }

    let emitted = model_emitted_events(model).map_err(|error| {
        format!(
            "failed to parse emitted events for `{}`: {error}",
            model.name
        )
    })?;
    let model_name = model.name.as_str();
    let model_snake = to_snake_case(model_name);
    let event_alias = ident(&format!("{model_name}CreatedEvent"));
    let op_id = format!("model.{model_name}.subscribe");
    let canonical_path = format!("/rpc/subscribe/{op_id}");

    let registrations = emitted.iter().map(|operation| {
        let method_ident = ident(&format!("on_{model_snake}_{}", operation.as_str()));
        quote! {
            {
                let push = push.clone();
                let handle = events.#method_ident(move |event| {
                    push.push(event);
                    ::core::future::ready(Ok(()))
                });
                guard.track(handle);
            }
        }
    });

    Ok(Some(quote! {
        #op_id => {
            if let Err(error) = ::cratestack::__private::validate_subscribe_accept_header(&headers) {
                return rpc_dispatch_error(&state, &headers, error);
            }
            let request = request_context("GET", #canonical_path, None, &headers, &[], &client_ip_ctx.extensions);
            let _ctx = match state.auth_provider.authenticate(&request).await {
                Ok(ctx) => ::cratestack::enrich_context_from_headers(ctx, &headers, client_ip_ctx.trusted_proxy.as_ref(), client_ip_ctx.peer),
                Err(error) => {
                    let error: ::cratestack::CoolError = error.into();
                    return rpc_dispatch_error(&state, &headers, error);
                }
            };
            let events = state.db.events();
            let mut guard = ::cratestack::SubscriptionGuard::new(events.__event_bus());
            let (push, rx) =
                ::cratestack::__private::subscription_channel::<super::events::#event_alias>();
            #(#registrations)*
            drop(push);
            let stream = ::cratestack::__private::guarded_receiver_stream(rx, guard);
            ::cratestack::__private::encode_model_event_sse_response(stream)
        }
    }))
}
