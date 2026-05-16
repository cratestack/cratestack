//! Top-level relation order/filter module emission, plus the
//! to-many `some`/`every`/`none` quantifier dispatch container.

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::model::FieldModuleKind;
use crate::shared::{find_model, ident, rust_type_tokens, to_snake_case};

use super::order_recursive::generate_relation_order_module_recursive;
use super::parse::parse_relation_attribute;
use super::types::{
    RelationFilterWrapperKind, RelationPathSegment, relation_link, relation_visit_key,
};

pub(crate) fn generate_relation_order_module(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    kind: FieldModuleKind,
) -> Result<proc_macro2::TokenStream, String> {
    let relation_link = relation_link(model, relation_field, models)?;
    let root_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;

    if relation_link.is_to_many {
        return generate_relation_quantifier_container_module(
            model,
            root_model,
            relation_field,
            &[],
            &[relation_visit_key(model, relation_field)],
            models,
        );
    }

    let wrappers = vec![RelationPathSegment {
        link: relation_link.clone(),
        kind: RelationFilterWrapperKind::ToOne,
    }];
    let visited = vec![relation_visit_key(model, relation_field)];

    // `as_include()` references `super::*_MODEL` descriptors, which
    // the client schema (`include_client_schema!`) doesn't emit. Skip
    // it on the client path — `RelationInclude` is a server-side ORM
    // concept and isn't reachable from generated client code anyway.
    let as_include_method = match kind {
        FieldModuleKind::Server => {
            generate_as_include_method(model, relation_field, root_model, models)?
        }
        FieldModuleKind::Client => None,
    };
    let root_extra: Vec<proc_macro2::TokenStream> = as_include_method.into_iter().collect();

    generate_relation_order_module_recursive(
        &relation_link,
        root_model,
        root_model,
        relation_link.related_table.as_str(),
        &[],
        relation_field,
        &wrappers,
        &visited,
        models,
        &root_extra,
    )
}

pub(super) fn generate_relation_quantifier_container_module(
    parent_model: &Model,
    target_model: &Model,
    relation_field: &Field,
    parent_wrappers: &[RelationPathSegment],
    visited: &[String],
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&relation_field.name);
    let some = super::quantifier::generate_relation_quantifier_module(
        parent_model, target_model, relation_field, parent_wrappers,
        RelationFilterWrapperKind::Some, "some", visited, models,
    )?;
    let every = super::quantifier::generate_relation_quantifier_module(
        parent_model, target_model, relation_field, parent_wrappers,
        RelationFilterWrapperKind::Every, "every", visited, models,
    )?;
    let none = super::quantifier::generate_relation_quantifier_module(
        parent_model, target_model, relation_field, parent_wrappers,
        RelationFilterWrapperKind::None, "none", visited, models,
    )?;

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Path;

            impl Path {
                pub fn some(self) -> self::some::Path {
                    self::some::Path
                }

                pub fn every(self) -> self::every::Path {
                    self::every::Path
                }

                pub fn none(self) -> self::none::Path {
                    self::none::Path
                }
            }

            #some
            #every
            #none
        }
    })
}

/// Emit the per-relation `as_include()` method body. Returns `None`
/// when the relation isn't eligible for the typed `.include(...)`
/// shortcut. Eligible shape: a to-one relation whose
/// `@relation(references:[<col>])` names the related model's primary
/// key.
fn generate_as_include_method(
    model: &Model,
    relation_field: &Field,
    related_model: &Model,
    _models: &[Model],
) -> Result<Option<proc_macro2::TokenStream>, String> {
    let Some(parsed) = parse_relation_attribute(relation_field) else {
        return Ok(None);
    };
    if parsed.fields.len() != 1 || parsed.references.len() != 1 {
        return Ok(None);
    }
    let fk_field_name = &parsed.fields[0];
    let ref_field_name = &parsed.references[0];

    let Some(related_pk) = related_model
        .fields
        .iter()
        .find(|field| crate::shared::is_primary_key(field))
    else {
        return Ok(None);
    };
    // Only support reference-equals-PK for v1.
    if ref_field_name != &related_pk.name {
        return Ok(None);
    }

    let Some(fk_field) = model.fields.iter().find(|field| &field.name == fk_field_name) else {
        return Ok(None);
    };

    let parent_ident = ident(&model.name);
    let related_ident = ident(&related_model.name);
    let related_pk_type = rust_type_tokens(&related_pk.ty);
    let related_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&related_model.name).to_uppercase(),
    ));
    let fk_field_ident = ident(&fk_field.name);

    // Optional FK ⇒ field type is already `Option<RelPK>`. Required
    // FK ⇒ wrap in `Some(...)` so the function pointer's return type
    // is the same shape in both cases.
    let fk_extract_body = if fk_field.ty.arity == TypeArity::Optional {
        quote! { m.#fk_field_ident.clone() }
    } else {
        quote! { ::std::option::Option::Some(m.#fk_field_ident.clone()) }
    };

    Ok(Some(quote! {
        /// Build a `RelationInclude` for this to-one relation.
        pub fn as_include(self) -> ::cratestack::RelationInclude<
            super::#parent_ident,
            super::#related_ident,
            #related_pk_type,
        > {
            ::cratestack::RelationInclude {
                parent_fk_extract: |m: &super::#parent_ident| #fk_extract_body,
                related_descriptor: &super::#related_descriptor_ident,
            }
        }
    }))
}
