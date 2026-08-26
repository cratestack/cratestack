//! `.route(...)` chain for the per-model handlers, mounted on the
//! generated `model_router`.
//!
//! Per-action `MethodRouter`s (cratestack#743, implementing
//! `docs/design/route-suppression.md`): each verb is built as its own
//! single-method `axum::routing::{get,post,patch,delete}(...)`
//! fragment and the survivors for a path are folded together with
//! `.merge()`, rather than one fused `.get(..).post(..)` chain. This
//! is what lets a suppressed verb (`@@internal(...)`,
//! `cratestack_core::model_internal_actions`) be omitted from the
//! merge instead of routed to a handler that would have to reject it
//! at runtime — suppression here is *emitting nothing*, not a new
//! runtime branch (design doc §3, §4). When every verb on a path is
//! suppressed, `merge_method_routes` returns `None` and the whole
//! `.route(path, ...)` call is omitted, so the path is never
//! registered at all — axum's own default 404 applies, no
//! `Router::fallback` needed. When nothing is suppressed this folds
//! back to routing every verb, matching today's behavior.

#[cfg(test)]
mod tests;

use cratestack_core::Model;
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

pub(crate) fn generate_model_axum_routes(model: &Model) -> proc_macro2::TokenStream {
    let snake = to_snake_case(&model.name);
    let plural = pluralize(&snake);
    let list_route = format!("/{}", plural);
    let detail_route = format!("/{}/{{id}}", plural);
    let list_handler_ident = ident(&format!("handle_list_{}", plural));
    let create_handler_ident = ident(&format!("handle_create_{}", plural));
    let get_handler_ident = ident(&format!("handle_get_{}", snake));
    let update_handler_ident = ident(&format!("handle_update_{}", snake));
    let delete_handler_ident = ident(&format!("handle_delete_{}", snake));

    let internal = cratestack_core::model_internal_actions(model);

    let list_router = merge_method_routes(&[
        (
            !internal.contains("list"),
            quote! { axum::routing::get(#list_handler_ident) },
        ),
        (
            !internal.contains("create"),
            quote! { axum::routing::post(#create_handler_ident) },
        ),
    ])
    .map(|router| quote! { .route(#list_route, #router) })
    .unwrap_or_default();

    let detail_router = merge_method_routes(&[
        (
            !internal.contains("get"),
            quote! { axum::routing::get(#get_handler_ident) },
        ),
        (
            !internal.contains("update"),
            quote! { axum::routing::patch(#update_handler_ident) },
        ),
        (
            !internal.contains("delete"),
            quote! { axum::routing::delete(#delete_handler_ident) },
        ),
    ])
    .map(|router| quote! { .route(#detail_route, #router) })
    .unwrap_or_default();

    quote! {
        #list_router
        #detail_router
    }
}

/// Folds the `MethodRouter` fragments whose `keep` flag is `true` into
/// one merged `MethodRouter` via `.merge()`, in declaration order.
/// `None` when every fragment is suppressed — the path this would have
/// routed must not be registered at all (design doc §3/§4).
fn merge_method_routes(
    parts: &[(bool, proc_macro2::TokenStream)],
) -> Option<proc_macro2::TokenStream> {
    let mut survivors = parts
        .iter()
        .filter(|(keep, _)| *keep)
        .map(|(_, router)| router.clone());
    let first = survivors.next()?;
    Some(survivors.fold(first, |acc, next| quote! { #acc.merge(#next) }))
}
