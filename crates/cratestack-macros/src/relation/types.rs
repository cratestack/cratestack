//! Shared relation types — wrapper kinds for filter codegen, path
//! segments accumulated during recursive traversal, and the
//! `relation_link` resolver that turns a `@relation(...)` annotation
//! into a concrete (parent_table, parent_column, related_table,
//! related_column) tuple.

use cratestack_core::{Field, Model, TypeArity};

use crate::shared::{find_model, pluralize, to_snake_case};

#[derive(Clone)]
pub(crate) struct RelationLink {
    pub(crate) parent_table: String,
    pub(crate) parent_column: String,
    pub(crate) related_table: String,
    pub(crate) related_column: String,
    pub(crate) is_to_many: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum RelationFilterWrapperKind {
    ToOne,
    Some,
    Every,
    None,
}

#[derive(Clone)]
pub(crate) struct RelationPathSegment {
    pub(crate) link: RelationLink,
    pub(crate) kind: RelationFilterWrapperKind,
}

pub(super) type RelationModuleEntry = (proc_macro2::TokenStream, proc_macro2::TokenStream);

pub(crate) struct ParsedRelationAttribute {
    pub(crate) fields: Vec<String>,
    pub(crate) references: Vec<String>,
}

pub(crate) fn relation_visit_key(model: &Model, relation_field: &Field) -> String {
    format!("{}.{}", model.name, relation_field.name)
}

pub(crate) fn relation_link(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
) -> Result<RelationLink, String> {
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let parent_table = pluralize(&to_snake_case(&model.name));
    let related_table = pluralize(&to_snake_case(&target_model.name));
    let relation = super::parse::parse_relation_attribute(relation_field).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` must declare @relation(fields:[...],references:[...])",
            relation_field.name, model.name,
        )
    })?;
    if relation.fields.len() != 1 || relation.references.len() != 1 {
        return Err(format!(
            "relation field `{}` on `{}` must declare exactly one local field and one reference in this slice",
            relation_field.name, model.name,
        ));
    }

    let local_field = model
        .fields
        .iter()
        .find(|field| field.name == relation.fields[0])
        .ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown local field `{}`",
                relation_field.name, model.name, relation.fields[0],
            )
        })?;
    let target_field = target_model
        .fields
        .iter()
        .find(|field| field.name == relation.references[0])
        .ok_or_else(|| {
            format!(
                "relation field `{}` on `{}` references unknown target field `{}` on `{}`",
                relation_field.name, model.name, relation.references[0], target_model.name,
            )
        })?;
    if local_field.ty.name != target_field.ty.name {
        return Err(format!(
            "relation field `{}` on `{}` links incompatible scalar types: local field `{}` is `{}` but referenced field `{}` is `{}`",
            relation_field.name,
            model.name,
            local_field.name,
            local_field.ty.name,
            target_field.name,
            target_field.ty.name,
        ));
    }

    Ok(RelationLink {
        parent_table,
        parent_column: to_snake_case(&local_field.name),
        related_table,
        related_column: to_snake_case(&target_field.name),
        is_to_many: relation_field.ty.arity == TypeArity::List,
    })
}
