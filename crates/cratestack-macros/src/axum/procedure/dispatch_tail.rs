//! The final leg of the generated per-procedure dispatch fn: log the
//! outcome and encode the HTTP response, once `result` (the outcome of
//! `invoke_with_db`) is known. Two shapes, selected by
//! [`procedure_dispatch_tail_tokens`]:
//!
//! - Ordinary procedures, and non-`@stream` `T[]` ones: **unchanged**.
//!   `result: Result<Output, CratestackError>` is fully resolved — nothing has
//!   been sent to the client yet — so it's logged as `Ok`/`Err` and
//!   handed to the already-computed buffered `result_encoder` fragment.
//!   This branch's tokens are untouched by cratestack#283; see
//!   `crate::axum::procedure::tests` for the byte-identical regression
//!   guard this ticket's acceptance criteria require.
//! - `@stream` procedures: `result` is
//!   `Result<impl Stream<Item = Result<Item, CratestackError>>, CratestackError>`
//!   (see `super::invoke_call`) — nothing has been *produced* yet, only
//!   handed off, so logging "completed" against it the way the buffered
//!   branch does would be a lie about what actually happened. This
//!   branch logs a "stream dispatched" event instead (successful
//!   hand-off to the incremental encoder, not full production) and
//!   calls the async `encode_transport_stream_result_with_status_for`,
//!   which owns the genuinely incremental encode
//!   (`cratestack-axum::transport::stream_sequence`, cratestack#283) —
//!   including logging what happens for the rest of that stream's life
//!   (mid-stream failure, client disconnect), which this dispatch fn has
//!   no way to observe since it returns as soon as the `Body` is built.

mod compose_tail;

use std::collections::BTreeSet;

use cratestack_core::Procedure;
use quote::quote;

use compose_tail::compose_tail_tokens;

use crate::computed::procedure_output_composition;
use crate::shared::is_stream_procedure;

pub(super) fn procedure_dispatch_tail_tokens(
    procedure: &Procedure,
    procedure_name: &str,
    success_status: &proc_macro2::TokenStream,
    result_encoder: &proc_macro2::TokenStream,
    deprecation_header: &proc_macro2::TokenStream,
    bearing: &BTreeSet<String>,
) -> proc_macro2::TokenStream {
    if is_stream_procedure(procedure) {
        // `cratestack-parser` rejects a `@stream` procedure whose item
        // type is computed-bearing (`docs/design/computed-fields.md`'s
        // "Exclusions" section) — item-wise resolution inside the
        // incremental encoder isn't implemented, so a schema that made
        // it past validation can never reach this branch with a
        // composable output. Nothing to fork on here.
        let tracing_tail =
            result_tracing_tokens(procedure_name, "cratestack procedure stream dispatched");
        quote! {
            #tracing_tail
            let mut response = ::cratestack::encode_transport_stream_result_with_status_for(
                &state.codec,
                &headers,
                &CAPABILITIES,
                #success_status,
                result,
            ).await;
            #deprecation_header
            response
        }
    } else {
        let compose_tail = compose_tail_tokens(procedure_output_composition(
            &procedure.return_type,
            bearing,
        ));
        let tracing_tail =
            result_tracing_tokens(procedure_name, "cratestack procedure route completed");
        quote! {
            #compose_tail
            #tracing_tail
            let mut response = #result_encoder;
            #deprecation_header
            response
        }
    }
}

/// The `match &result { Ok(_) => info!(...), Err(_) => warn!(...) }`
/// block shared by both tails — only the `Ok` arm's message text
/// differs (`ok_message`); the `Err` arm ("...route failed") is
/// identical either way, since a preflight-time `Err` looks the same
/// regardless of whether the procedure is `@stream`.
fn result_tracing_tokens(procedure_name: &str, ok_message: &str) -> proc_macro2::TokenStream {
    quote! {
        match &result {
            Ok(_) => ::cratestack::tracing::info!(
                target: "cratestack",
                cratestack_route = canonical_route,
                cratestack_procedure = #procedure_name,
                cratestack_operation = "procedure",
                cratestack_authenticated = ctx.is_authenticated(),
                cratestack_duration_ms = started.elapsed().as_millis() as u64,
                cratestack_request_id = ctx.request_id().unwrap_or(""),
                #ok_message,
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
    }
}
