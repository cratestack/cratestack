//! Per-model `OrderCatalog` static emission for the REST `?orderBy=`/
//! `?sort=` dispatcher (cratestack#256).
//!
//! Replaces the old recursive `collect_relation_order_targets` walk,
//! which emitted one `(key, sql)` match arm per *distinct path* through
//! the to-one relation graph — exponential in graph connectivity, the
//! same shape as the codegen bug fixed in #252 for the typed builder.
//! Here every model emits exactly one `OrderCatalog` naming its own
//! scalar columns and its own to-one relation edges, so total emitted
//! code is linear in `models × fields` regardless of connectivity. The
//! dotted key is resolved hop by hop at request time by
//! `cratestack_sql::order_catalog::resolve_order_target` — see that
//! module for the runtime side.

use cratestack_core::Model;
use quote::quote;

use crate::shared::{
    ident, model_name_set, relation_model_fields, scalar_model_fields, to_snake_case,
};

use super::types::relation_link;

/// The `static` identifier for `model`'s `OrderCatalog`, e.g.
/// `POST_ORDER_CATALOG`. Shared by the catalog's own definition and by
/// every other model's relation edges that point at it.
pub(crate) fn order_catalog_ident(model_name: &str) -> syn::Ident {
    ident(&format!(
        "{}_ORDER_CATALOG",
        to_snake_case(model_name).to_uppercase()
    ))
}

/// Emits `static <MODEL>_ORDER_CATALOG: ::cratestack::OrderCatalog = ...;`
/// for `model`: its own scalar columns, plus one [`::cratestack::OrderRelationEdge`]
/// per to-one relation field pointing at the target model's own catalog.
/// To-many relations are skipped entirely, matching the old walk's
/// behaviour of never producing a sortable key through them.
pub(crate) fn generate_model_order_catalog(
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_names = model_name_set(models);
    let catalog_ident = order_catalog_ident(&model.name);

    let scalars = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let api_name = &field.name;
            let column = to_snake_case(&field.name);
            quote! { (#api_name, #column) }
        })
        .collect::<Vec<_>>();

    let relations = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|relation_field| {
            let link = relation_link(model, relation_field, models)?;
            if link.is_to_many {
                return Ok(None);
            }
            let api_name = &relation_field.name;
            let parent_table = link.parent_table.as_str();
            let parent_column = link.parent_column.as_str();
            let related_table = link.related_table.as_str();
            let related_column = link.related_column.as_str();
            let target_catalog_ident = order_catalog_ident(&relation_field.ty.name);
            Ok(Some(quote! {
                ::cratestack::OrderRelationEdge {
                    api_name: #api_name,
                    hop: ::cratestack::RelationHop::new(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        ::cratestack::RelationQuantifier::ToOne,
                    ),
                    target: &#target_catalog_ident,
                }
            }))
        })
        .collect::<Result<Vec<_>, String>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    Ok(quote! {
        static #catalog_ident: ::cratestack::OrderCatalog = ::cratestack::OrderCatalog {
            scalars: &[#(#scalars),*],
            relations: &[#(#relations),*],
        };
    })
}
