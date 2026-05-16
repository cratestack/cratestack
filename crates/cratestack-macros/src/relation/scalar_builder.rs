//! Per-scalar `Field` impl module inside a relation order/filter
//! module — collects the chained filter methods (eq/ne/in/...) and,
//! for to-one chains, the asc()/desc() order helpers.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{ident, rust_type_tokens, to_snake_case};

use super::filter_builders;
use super::order_targets::relation_order_value_sql_for_path;
use super::types::{RelationLink, RelationPathSegment};

#[allow(clippy::too_many_arguments)]
pub(super) fn generate_scalar_relation_builder_module(
    field: &Field,
    wrappers: &[RelationPathSegment],
    allow_ordering: bool,
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    path_prefix: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&field.name);
    let field_type = rust_type_tokens(&field.ty);
    let column = to_snake_case(&field.name);
    let mut methods = Vec::new();

    filter_builders::append_required_builder_methods(
        &mut methods, field, wrappers, &field_type, &column,
    );
    filter_builders::append_boolean_builder_methods(
        &mut methods, field, wrappers, &field_type, &column,
    );
    filter_builders::append_required_text_builder_methods(
        &mut methods, field, wrappers, &field_type, &column,
    );
    filter_builders::append_optional_builder_methods(
        &mut methods, field, wrappers, &field_type, &column,
    );
    filter_builders::append_optional_string_builder_methods(
        &mut methods, field, wrappers, &field_type, &column,
    );

    if allow_ordering {
        let mut path = path_prefix.to_vec();
        path.push(field.name.clone());
        let value_sql = relation_order_value_sql_for_path(root_model, models, root_table, &path)?;
        let parent_table = root_link.parent_table.as_str();
        let parent_column = root_link.parent_column.as_str();
        let related_table = root_link.related_table.as_str();
        let related_column = root_link.related_column.as_str();
        methods.push(quote! {
            pub fn asc(self) -> ::cratestack::OrderClause {
                ::cratestack::OrderClause::relation_scalar(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #value_sql,
                    ::cratestack::SortDirection::Asc,
                )
            }
        });
        methods.push(quote! {
            pub fn desc(self) -> ::cratestack::OrderClause {
                ::cratestack::OrderClause::relation_scalar(
                    #parent_table,
                    #parent_column,
                    #related_table,
                    #related_column,
                    #value_sql,
                    ::cratestack::SortDirection::Desc,
                )
            }
        });
    }

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Field;

            impl Field {
                #(#methods)*
            }
        }
    })
}
