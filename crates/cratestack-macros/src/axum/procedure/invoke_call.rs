//! Bridges the `ProcedureRegistry` trait method's return shape — a
//! `Future` for ordinary procedures, a `Stream` for `@stream`-marked ones
//! (see `crate::procedure::generate_procedure_registry_method`) — into
//! the `Result<Output, CratestackError>` shape `invoke_with_db`'s closure
//! needs to return.
//!
//! For ordinary procedures `Output` is the plain return type and this is
//! a direct `.await`.
//!
//! For `@stream` procedures this is *not* simply `Ok(registry
//! .#method_ident(&db, &call_ctx, call_args))`, even though that method
//! call itself is synchronous (no `.await` needed to obtain the
//! `Stream`) — return-position `impl Trait` in traits captures *every*
//! in-scope lifetime by default (unlike ordinary free-fn `impl Trait`),
//! so the returned `impl Stream<..> + Send`'s type is only valid as long
//! as the `&db`/`&call_ctx` borrows passed into it are. `db`/`call_ctx`
//! are local to the closure `invoke_with_db` calls — once that closure's
//! `async move` block finishes evaluating (handing its `Result` back to
//! `invoke_with_db`), a `Stream` value that merely *borrowed* `db`/
//! `call_ctx` would be dangling: the stream travels on into the HTTP
//! response body, potentially long after this dispatch function's own
//! stack frame is gone.
//!
//! The fix: wrap the call in a `async_stream::stream!` generator that
//! *owns* `db`/`call_ctx`/`registry`/`call_args` (moved in at
//! construction) and, internally, holds the borrowing inner stream as
//! part of its own generator state — sound because it's all one
//! self-contained state machine (this is exactly what `async-stream`
//! exists for; the same pattern `examples/rpc-streaming`'s own `ticks`
//! implementation already uses, just nested one level deeper here). The
//! resulting generator is a single value that owns everything it needs,
//! so it can freely outlive this function. `invoke_with_db` still runs
//! `authorize_with_db` first, so a `@stream` procedure's authorization
//! gate is exactly as effective as any other procedure's — only the
//! `f()` call itself is now "construct the (self-contained) stream", not
//! "produce the whole result".
//!
//! This used to (`try_collect::<Vec<_>>().await`) buffer every item
//! before returning — that was deliberately identical to the
//! pre-`@stream` behavior, cratestack#282's own non-breaking
//! requirement. cratestack#283 (this ticket) replaces that buffering
//! with the genuinely incremental HTTP encoding in
//! `cratestack-axum::transport::stream_sequence`, which consumes the
//! `Stream` returned here directly — see
//! `crate::axum::procedure::dispatch_tail` for where the two paths
//! (buffered vs. streamed) fork after this call.

use cratestack_core::Procedure;
use quote::quote;

use crate::shared::is_stream_procedure;

/// Token stream for the call inside `invoke_with_db`'s closure that
/// actually invokes the registry method: `registry.<method>(&db, &ctx,
/// args, authorized)`, either `.await`ed directly (ordinary procedures)
/// or wrapped in a self-owning generator stream (`@stream` procedures —
/// see the module doc for why a direct `Ok(registry.#method_ident(..))`
/// isn't sound here). `authorized` is the closure parameter
/// `invoke_with_db` (cratestack#512) hands in — the `Authorized` witness
/// that only its own `authorize_with_db` call could have constructed, and
/// the sole reason this call site (unlike any code outside the closure)
/// is allowed to make it at all.
pub(super) fn procedure_invoke_call_tokens(
    procedure: &Procedure,
    method_ident: &syn::Ident,
) -> proc_macro2::TokenStream {
    if is_stream_procedure(procedure) {
        quote! {
            Ok(::cratestack::async_stream::stream! {
                let mut __cratestack_stream_source =
                    ::std::boxed::Box::pin(registry.#method_ident(&db, &call_ctx, call_args, authorized));
                while let Some(__cratestack_stream_item) =
                    ::cratestack::futures::StreamExt::next(&mut __cratestack_stream_source).await
                {
                    yield __cratestack_stream_item;
                }
            })
        }
    } else {
        quote! {
            registry.#method_ident(&db, &call_ctx, call_args, authorized).await
        }
    }
}
