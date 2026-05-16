//! `wrap_filter_expr_tokens` — wraps a scalar `FilterExpr` in nested
//! `FilterExpr::relation{,_some,_every,_none}` calls per the accumulated
//! relation path. Lives at the bottom of the dependency tree so all
//! filter emitters can pull it in.

use quote::quote;

use super::types::{RelationFilterWrapperKind, RelationPathSegment};

pub(crate) fn wrap_filter_expr_tokens(
    base_expr: proc_macro2::TokenStream,
    wrappers: &[RelationPathSegment],
) -> proc_macro2::TokenStream {
    wrappers.iter().rev().fold(base_expr, |inner, wrapper| {
        let parent_table = wrapper.link.parent_table.as_str();
        let parent_column = wrapper.link.parent_column.as_str();
        let related_table = wrapper.link.related_table.as_str();
        let related_column = wrapper.link.related_column.as_str();
        match wrapper.kind {
            RelationFilterWrapperKind::ToOne => quote! {
                ::cratestack::FilterExpr::relation(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::Some => quote! {
                ::cratestack::FilterExpr::relation_some(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::Every => quote! {
                ::cratestack::FilterExpr::relation_every(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
            RelationFilterWrapperKind::None => quote! {
                ::cratestack::FilterExpr::relation_none(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #inner,
                )
            },
        }
    })
}
