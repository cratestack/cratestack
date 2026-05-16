//! SQL-fragment computation for sorting through to-one relations.
//! `collect_relation_order_targets` walks the model graph collecting
//! every `(api_key, sql_fragment)` pair reachable through to-one
//! relations; `relation_order_value_sql_for_path` computes one such
//! fragment for a specific dotted path.

use cratestack_core::Model;

use crate::shared::{
    find_model, is_relation_field, model_name_set, relation_model_fields, scalar_model_fields,
    to_snake_case,
};

use super::types::{relation_link, relation_visit_key};

pub(crate) fn collect_relation_order_targets(
    model: &Model,
    models: &[Model],
    current_table: &str,
    prefix: &str,
) -> Result<Vec<(String, String)>, String> {
    collect_inner(model, models, current_table, prefix, &[])
}

fn collect_inner(
    model: &Model,
    models: &[Model],
    current_table: &str,
    prefix: &str,
    visited: &[String],
) -> Result<Vec<(String, String)>, String> {
    let model_names = model_name_set(models);
    let mut targets = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            (
                format!("{}.{}", prefix, field.name),
                format!("{}.{}", current_table, to_snake_case(&field.name)),
            )
        })
        .collect::<Vec<_>>();

    for relation_field in relation_model_fields(model, &model_names) {
        let visit_key = relation_visit_key(model, relation_field);
        if visited.contains(&visit_key) {
            continue;
        }
        let relation_link = relation_link(model, relation_field, models)?;
        if relation_link.is_to_many {
            continue;
        }
        let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown model `{}`",
                relation_field.name, model.name, relation_field.ty.name,
            )
        })?;
        let mut next_visited = visited.to_vec();
        next_visited.push(visit_key);
        let nested_targets = collect_inner(
            target_model,
            models,
            relation_link.related_table.as_str(),
            &format!("{}.{}", prefix, relation_field.name),
            &next_visited,
        )?;
        targets.extend(nested_targets.into_iter().map(|(key, nested_sql)| {
            (
                key,
                format!(
                    "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1)",
                    nested_sql,
                    relation_link.related_table,
                    relation_link.related_table,
                    relation_link.related_column,
                    current_table,
                    relation_link.parent_column,
                ),
            )
        }));
    }

    Ok(targets)
}

pub(super) fn relation_order_value_sql_for_path(
    model: &Model,
    models: &[Model],
    current_table: &str,
    path: &[String],
) -> Result<String, String> {
    let Some((segment, rest)) = path.split_first() else {
        return Err(format!(
            "empty relation order path on model `{}`",
            model.name
        ));
    };
    let field = model
        .fields
        .iter()
        .find(|field| field.name == *segment)
        .ok_or_else(|| format!("unknown field `{segment}` on model `{}`", model.name))?;
    let model_names = model_name_set(models);

    if !is_relation_field(&model_names, field) {
        if !rest.is_empty() {
            return Err(format!(
                "scalar field `{}` on model `{}` cannot continue relation order path",
                field.name, model.name,
            ));
        }
        return Ok(format!("{}.{}", current_table, to_snake_case(&field.name)));
    }

    let relation_link = relation_link(model, field, models)?;
    if relation_link.is_to_many {
        return Err(format!(
            "relation field `{}` on `{}` cannot be used in orderBy because it is to-many",
            field.name, model.name,
        ));
    }
    if rest.is_empty() {
        return Err(format!(
            "relation field `{}` on `{}` must target a scalar field for orderBy",
            field.name, model.name,
        ));
    }

    let target_model = find_model(models, &field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            field.name, model.name, field.ty.name,
        )
    })?;
    let nested_sql = relation_order_value_sql_for_path(
        target_model,
        models,
        relation_link.related_table.as_str(),
        rest,
    )?;

    Ok(format!(
        "(SELECT {} FROM {} WHERE {}.{} = {}.{} LIMIT 1)",
        nested_sql,
        relation_link.related_table,
        relation_link.related_table,
        relation_link.related_column,
        current_table,
        relation_link.parent_column,
    ))
}
