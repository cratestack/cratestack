//! Shared relation types — the `relation_link` resolver that turns a `@relation(...)` annotation
//! into a concrete (parent_table, parent_column, related_table,
//! related_column) tuple.

use cratestack_core::{Field, Model, TypeArity};

use quote::quote;

use crate::shared::{find_model, ident, pluralize, to_snake_case};

#[derive(Clone)]
pub(crate) struct RelationLink {
    pub(crate) parent_table: String,
    pub(crate) parent_column: String,
    pub(crate) related_table: String,
    pub(crate) related_column: String,
    pub(crate) is_to_many: bool,
}

pub(crate) struct ParsedRelationAttribute {
    pub(crate) fields: Vec<String>,
    pub(crate) references: Vec<String>,
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

/// `<prefix><TARGET>_MODEL.related_read_scope()` — the related model's
/// read scope, spliced into every relation subquery that reads its table
/// (GHSA-p55v-6xv5-93p3). `prefix` is the `super::`-path from the emission
/// site to the schema root, where the `*_MODEL` descriptors live.
pub(crate) fn related_scope_tokens(
    target_model_name: &str,
    prefix: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let descriptor = ident(&format!(
        "{}_MODEL",
        to_snake_case(target_model_name).to_uppercase()
    ));
    quote! { #prefix #descriptor.related_read_scope() }
}

/// [`related_scope_tokens`] for a field-module emission site. The client
/// role emits no `*_MODEL` descriptors and has no SQL backend (nothing in
/// the client SDK renders a `FilterExpr`; a server re-derives its own
/// scoped filters from the wire), so it passes the explicit
/// `RelatedReadScope::Unscoped`.
pub(crate) fn field_module_scope_tokens(
    kind: crate::model::FieldModuleKind,
    target_model_name: &str,
    prefix: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match kind {
        crate::model::FieldModuleKind::Server => related_scope_tokens(target_model_name, prefix),
        crate::model::FieldModuleKind::Client => {
            quote! { ::cratestack::RelatedReadScope::Unscoped }
        }
    }
}
