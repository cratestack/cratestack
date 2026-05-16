//! Resolution of `a.b.c` policy paths through `@relation`s into
//! flattened segments + a final scalar target field. The segments
//! capture quantifier semantics (`some`/`every`/`none` for to-many,
//! `to_one` for to-one) which [`super::predicates::wrap_relation_predicate`]
//! later folds back into nested `ReadPredicate::Relation` nodes.

use cratestack_core::{Field, Model, TypeArity};

use crate::relation::{RelationLink, relation_link};
use crate::shared::{find_model, is_relation_field, model_name_set, to_snake_case};

use super::predicates::find_model_field;

#[derive(Clone)]
pub(super) struct RelationPolicySegment {
    pub(super) link: RelationLink,
    pub(super) quantifier: &'static str,
}

pub(super) struct RelationPolicyField<'a> {
    pub(super) relations: Vec<RelationPolicySegment>,
    pub(super) target_field: &'a Field,
    pub(super) target_column: String,
}

pub(super) fn resolve_relation_policy_field<'a>(
    model: &'a Model,
    models: &'a [Model],
    path: &str,
) -> Result<Option<RelationPolicyField<'a>>, String> {
    if path.starts_with("auth().") || !path.contains('.') {
        return Ok(None);
    }

    let model_names = model_name_set(models);
    let mut current_model = model;
    let mut relations = Vec::new();
    let parts = path.split('.').collect::<Vec<_>>();
    let mut index = 0usize;
    while let Some(part) = parts.get(index).copied() {
        let field = find_model_field(current_model, part)?;
        if !is_relation_field(&model_names, field) {
            if index + 1 != parts.len() {
                return Err(format!(
                    "relation policy path `{path}` cannot traverse through scalar field `{part}`"
                ));
            }
            return Ok(Some(RelationPolicyField {
                relations,
                target_field: field,
                target_column: to_snake_case(&field.name),
            }));
        }

        if index + 1 == parts.len() {
            return Err(format!(
                "relation policy path `{path}` must end on a scalar field"
            ));
        }

        let link = relation_link(current_model, field, models)?;
        let (quantifier, step) = if field.ty.arity == TypeArity::List {
            let quantifier = match parts.get(index + 1).copied() {
                Some("some") => "some",
                Some("every") => "every",
                Some("none") => "none",
                Some(segment) => {
                    return Err(format!(
                        "relation policy path `{path}` must use `some`, `every`, or `none` after to-many relation `{part}`; found `{segment}`"
                    ));
                }
                None => {
                    return Err(format!(
                        "relation policy path `{path}` must use `some`, `every`, or `none` after to-many relation `{part}`"
                    ));
                }
            };
            (quantifier, 2usize)
        } else {
            if matches!(
                parts.get(index + 1).copied(),
                Some("some" | "every" | "none")
            ) {
                return Err(format!(
                    "relation policy path `{path}` cannot use a collection quantifier after to-one relation `{part}`"
                ));
            }
            ("to_one", 1usize)
        };

        relations.push(RelationPolicySegment { link, quantifier });
        current_model = find_model(models, &field.ty.name).ok_or_else(|| {
            format!(
                "relation policy path `{path}` references unknown target model `{}`",
                field.ty.name
            )
        })?;

        index += step;
        if quantifier != "to_one" && index >= parts.len() {
            return Err(format!(
                "relation policy path `{path}` must continue after `{part}.{quantifier}`"
            ));
        }
    }

    Ok(None)
}
