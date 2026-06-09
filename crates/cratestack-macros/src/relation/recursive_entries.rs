//! Walk a model's relation fields and produce `(path_method, module)`
//! pairs to splice into the parent order module. Cycle-broken via the
//! `visited` set carrying `model.field` keys. Used both by the
//! top-level recursive emitter and by the to-many quantifier modules.

use cratestack_core::{Field, Model};

use crate::shared::find_model;

use super::order_module::generate_relation_quantifier_container_module;
use super::order_recursive::generate_relation_order_module_recursive;
use super::path_method::generate_nested_relation_path_method;
use super::types::{
    RelationFilterWrapperKind, RelationLink, RelationModuleEntry, RelationPathSegment,
    relation_link, relation_visit_key,
};

pub(super) fn find_model_or_err<'a>(
    current_model: &Model,
    relation_field: &Field,
    models: &'a [Model],
) -> Result<&'a Model, String> {
    find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, current_model.name, relation_field.ty.name,
        )
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_recursive_relation_entry(
    current_model: &Model,
    nested_relation: &Field,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    path_prefix: &[String],
    root_link: &RelationLink,
    root_model: &Model,
    root_table: &str,
    models: &[Model],
) -> Result<Option<RelationModuleEntry>, String> {
    let nested_link = relation_link(current_model, nested_relation, models)?;
    let nested_key = relation_visit_key(current_model, nested_relation);
    if visited.contains(&nested_key) {
        return Ok(None);
    }

    if nested_link.is_to_many {
        let nested_model = find_model_or_err(current_model, nested_relation, models)?;
        let module = generate_relation_quantifier_container_module(
            current_model,
            nested_model,
            nested_relation,
            wrappers,
            visited,
            models,
        )?;
        return Ok(Some((
            generate_nested_relation_path_method(nested_relation),
            module,
        )));
    }

    let nested_model = find_model_or_err(current_model, nested_relation, models)?;
    let mut nested_path = path_prefix.to_vec();
    nested_path.push(nested_relation.name.clone());
    let mut nested_wrappers = wrappers.to_vec();
    nested_wrappers.push(RelationPathSegment {
        link: nested_link,
        kind: RelationFilterWrapperKind::ToOne,
    });
    let mut nested_visited = visited.to_vec();
    nested_visited.push(nested_key);
    let module = generate_relation_order_module_recursive(
        root_link,
        root_model,
        nested_model,
        root_table,
        &nested_path,
        nested_relation,
        &nested_wrappers,
        &nested_visited,
        models,
        &[],
    )?;
    Ok(Some((
        generate_nested_relation_path_method(nested_relation),
        module,
    )))
}

pub(super) fn build_quantifier_relation_entry(
    target_model: &Model,
    nested_relation: &Field,
    visited: &[String],
    wrappers: &[RelationPathSegment],
    models: &[Model],
) -> Result<Option<RelationModuleEntry>, String> {
    let nested_key = relation_visit_key(target_model, nested_relation);
    if visited.contains(&nested_key) {
        return Ok(None);
    }
    let mut nested_visited = visited.to_vec();
    nested_visited.push(nested_key);
    let nested_model = find_model_or_err(target_model, nested_relation, models)?;
    let nested_link = relation_link(target_model, nested_relation, models)?;

    if nested_link.is_to_many {
        let module = generate_relation_quantifier_container_module(
            target_model,
            nested_model,
            nested_relation,
            wrappers,
            &nested_visited,
            models,
        )?;
        return Ok(Some((
            generate_nested_relation_path_method(nested_relation),
            module,
        )));
    }

    let root_link = wrappers[0].link.clone();
    let mut nested_wrappers = wrappers.to_vec();
    nested_wrappers.push(RelationPathSegment {
        link: nested_link,
        kind: RelationFilterWrapperKind::ToOne,
    });
    let module = generate_relation_order_module_recursive(
        &root_link,
        target_model,
        nested_model,
        root_link.related_table.as_str(),
        std::slice::from_ref(&nested_relation.name),
        nested_relation,
        &nested_wrappers,
        &nested_visited,
        models,
        &[],
    )?;
    Ok(Some((
        generate_nested_relation_path_method(nested_relation),
        module,
    )))
}
