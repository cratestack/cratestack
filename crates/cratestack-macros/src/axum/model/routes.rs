//! `.route(...)` chain for the per-model handlers, mounted on the
//! generated `model_router`.

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

    quote! {
        .route(
            #list_route,
            axum::routing::get(#list_handler_ident).post(#create_handler_ident),
        )
        .route(
            #detail_route,
            axum::routing::get(#get_handler_ident)
                .patch(#update_handler_ident)
                .delete(#delete_handler_ident),
        )
    }
}
