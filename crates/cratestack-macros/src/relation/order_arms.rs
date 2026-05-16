//! `orderBy` arm emission for relation fields, plus the
//! `collect_allowed_sort_keys` helper consumed by the descriptor's
//! allow-list.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::find_model;

use super::order_targets::collect_relation_order_targets;
use super::types::relation_link;
use crate::shared::{pluralize, to_snake_case};

pub(crate) fn collect_allowed_sort_keys(
    model: &Model,
    models: &[Model],
) -> Result<Vec<String>, String> {
    let table_name = pluralize(&to_snake_case(&model.name));
    collect_relation_order_targets(model, models, &table_name, "").map(|targets| {
        targets
            .into_iter()
            .filter_map(|(key, _)| key.strip_prefix('.').map(str::to_owned))
            .collect()
    })
}

pub(crate) fn generate_relation_order_by_arms(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let arms = collect_relation_order_by_arms(model, relation_field, models, None)?;
    Ok(quote! { #(#arms)* })
}

fn collect_relation_order_by_arms(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    prefix: Option<&str>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    let relation_link = relation_link(model, relation_field, models)?;
    if relation_link.is_to_many {
        return Ok(Vec::new());
    }

    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let key_prefix = match prefix {
        Some(prefix) => format!("{}.{}", prefix, relation_field.name),
        None => relation_field.name.clone(),
    };
    let targets = collect_relation_order_targets(
        target_model,
        models,
        relation_link.related_table.as_str(),
        &key_prefix,
    )?;

    Ok(targets
        .into_iter()
        .map(|(key, value_sql)| {
            let parent_table = relation_link.parent_table.as_str();
            let parent_column = relation_link.parent_column.as_str();
            let related_table = relation_link.related_table.as_str();
            let related_column = relation_link.related_column.as_str();
            quote! {
                #key => {
                    request.order_by(::cratestack::OrderClause::relation_scalar(
                        #parent_table,
                        #parent_column,
                        #related_table,
                        #related_column,
                        #value_sql,
                        if descending {
                            ::cratestack::SortDirection::Desc
                        } else {
                            ::cratestack::SortDirection::Asc
                        },
                    ))
                }
            }
        })
        .collect())
}
