//! `generate_relation_query_guard` — emits the prefix-match arm that
//! routes `key.startswith("<relation>.")` into the target model's
//! filter builder. To-many relations gate on `some`/`every`/`none`
//! before the nested filter; to-one relations forward directly.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{find_model, ident, to_snake_case};

use super::types::relation_link;

pub(crate) fn generate_relation_query_guard(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let target_filter_builder_ident = ident(&format!(
        "build_{}_filter_expr",
        to_snake_case(&target_model.name)
    ));
    let relation_prefix = format!("{}.", relation_field.name);
    let relation_link = relation_link(model, relation_field, models)?;
    let parent_table = relation_link.parent_table;
    let parent_column = relation_link.parent_column;
    let related_table = relation_link.related_table;
    let related_column = relation_link.related_column;

    if relation_link.is_to_many {
        let relation_field_name = &relation_field.name;

        return Ok(quote! {
            if let Some(rest) = key.strip_prefix(#relation_prefix) {
                let (operator, nested_key) = rest.split_once('.').ok_or_else(|| {
                    CoolError::BadRequest(format!(
                        "to-many relation filter '{}.{}' must use one of some, every, or none before the target field",
                        #model_name,
                        #relation_field_name,
                    ))
                })?;
                return match operator {
                    "some" => Ok(::cratestack::FilterExpr::relation_some(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    "every" => Ok(::cratestack::FilterExpr::relation_every(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    "none" => Ok(::cratestack::FilterExpr::relation_none(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #target_filter_builder_ident(nested_key, value)?,
                    )),
                    _ => Err(CoolError::BadRequest(format!(
                        "unsupported to-many relation operator '{}' for {}.{}; expected some, every, or none",
                        operator,
                        #model_name,
                        #relation_field_name,
                    ))),
                };
            }
        });
    }

    Ok(quote! {
        if let Some(rest) = key.strip_prefix(#relation_prefix) {
            return Ok(::cratestack::FilterExpr::relation(
                #parent_table,
                #parent_column,
                #related_table,
                #related_column,
                #target_filter_builder_ident(rest, value)?,
            ));
        }
    })
}
