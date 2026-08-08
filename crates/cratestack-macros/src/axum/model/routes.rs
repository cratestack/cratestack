//! `.route(...)` chain for the per-model handlers, mounted on the
//! generated `model_router`.
//!
//! SPIKE (`spike/b1-internal-actions`): route emission is now
//! per-action rather than one fused `quote!`. `@@internal("update")`
//! on a model suppresses `PATCH /<plural>/{id}` while leaving the
//! model's `update` policy compiled and enforced exactly as before —
//! server code (procedures, workers) can still call
//! `db.model().update(..)`, and the policy still decides whether it
//! may. If every verb on a path is suppressed, the `.route(...)` for
//! that path is omitted entirely rather than mounted empty.

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use cratestack_core::{Model, model_internal_actions};
use quote::quote;

use crate::shared::{ident, pluralize, to_snake_case};

/// Actions whose REST route this model suppresses. Public to the
/// crate so handler generation can mark the now-unreferenced handler
/// fn `#[allow(dead_code)]` (the handler is still emitted — only the
/// mount is dropped).
pub(crate) fn suppressed_rest_actions(model: &Model) -> BTreeSet<String> {
    model_internal_actions(model)
}

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

    let suppressed = suppressed_rest_actions(model);
    let emits = |action: &str| !suppressed.contains(action);

    // Collection path: GET (list) + POST (create).
    let mut collection = Vec::new();
    if emits("list") {
        collection.push(quote! { axum::routing::get(#list_handler_ident) });
    }
    if emits("create") {
        collection.push(quote! { axum::routing::post(#create_handler_ident) });
    }

    // Detail path: GET (detail) + PATCH (update) + DELETE.
    let mut detail = Vec::new();
    if emits("detail") {
        detail.push(quote! { axum::routing::get(#get_handler_ident) });
    }
    if emits("update") {
        detail.push(quote! { axum::routing::patch(#update_handler_ident) });
    }
    if emits("delete") {
        detail.push(quote! { axum::routing::delete(#delete_handler_ident) });
    }

    let collection_route = merge_method_routes(&list_route, collection);
    let detail_route = merge_method_routes(&detail_route, detail);

    quote! {
        #collection_route
        #detail_route
    }
}

/// Fold the surviving `MethodRouter`s for one path into a single
/// `.route(path, a.merge(b).merge(c))`, or nothing at all when every
/// verb on that path was suppressed.
///
/// `merge` rather than the chained `get(..).post(..)` builder because
/// the chained form cannot express "start from whichever verb happens
/// to be first" without a match on every subset.
fn merge_method_routes(
    path: &str,
    method_routers: Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let mut iter = method_routers.into_iter();
    let Some(first) = iter.next() else {
        return proc_macro2::TokenStream::new();
    };
    let rest = iter.collect::<Vec<_>>();
    quote! {
        .route(
            #path,
            #first #( .merge(#rest) )*,
        )
    }
}
