//! Per-(model, to-one relation) `Root` entry type.
//!
//! `RelPath` is shared per *target model*, so it cannot carry
//! `as_include()` — that method needs the *parent* model's FK field and the
//! related model's PK, i.e. it is per-(model, relation field). `Root` holds
//! it, and forwards every other accessor into the shared `RelPath`, so
//! `post::author().as_include()` and
//! `post::author().profile().nickname().desc()` both keep working.
//!
//! Forwarding costs one method per field of the target model per relation —
//! linear in schema size, unlike the per-path tree it replaces.

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::model::FieldModuleKind;
use crate::shared::{
    find_model, ident, model_name_set, relation_model_fields, rust_type_tokens,
    scalar_model_fields, to_snake_case,
};

use super::parse::parse_relation_attribute;
use super::types::relation_link;

/// Emit `pub mod <relation_field> { pub struct Root; .. }` for a to-one
/// relation. To-many roots need no module: the model's field fn returns the
/// target's `RelToMany` directly.
pub(crate) fn generate_relation_root_module(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    kind: FieldModuleKind,
) -> Result<Option<proc_macro2::TokenStream>, String> {
    let link = relation_link(model, relation_field, models)?;
    if link.is_to_many {
        return Ok(None);
    }
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;

    let module_ident = ident(&relation_field.name);
    let target_module = ident(&to_snake_case(&target_model.name));
    let parent_table = link.parent_table.as_str();
    let parent_column = link.parent_column.as_str();
    let related_table = link.related_table.as_str();
    let related_column = link.related_column.as_str();

    let model_names = model_name_set(models);
    let mut forwards = Vec::new();
    for field in scalar_model_fields(target_model, &model_names) {
        let method = ident(&field.name);
        forwards.push(quote! {
            #[allow(non_snake_case)]
            pub fn #method(self) -> super::super::#target_module::#method::Field<::cratestack::Orderable> {
                Self::__path().#method()
            }
        });
    }
    for field in relation_model_fields(target_model, &model_names) {
        let nested_link = relation_link(target_model, field, models)?;
        let method = ident(&field.name);
        let nested_target = ident(&to_snake_case(&field.ty.name));
        let ret = if nested_link.is_to_many {
            quote! { super::super::#nested_target::RelToMany }
        } else {
            quote! { super::super::#nested_target::RelPath<::cratestack::Orderable> }
        };
        forwards.push(quote! {
            #[allow(non_snake_case)]
            pub fn #method(self) -> #ret {
                Self::__path().#method()
            }
        });
    }

    let as_include = match kind {
        FieldModuleKind::Server => generate_as_include_method(model, relation_field, target_model)?,
        FieldModuleKind::Client => None,
    };
    let as_include: Vec<proc_macro2::TokenStream> = as_include.into_iter().collect();

    Ok(Some(quote! {
        pub mod #module_ident {
            pub use super::*;

            pub struct Root;

            impl Root {
                fn __path() -> super::super::#target_module::RelPath<::cratestack::Orderable> {
                    super::super::#target_module::RelPath::__from_hops(
                        ::std::vec::Vec::from([
                            ::cratestack::RelationHop::new(
                                #parent_table,
                                #parent_column,
                                #related_table,
                                #related_column,
                                ::cratestack::RelationQuantifier::ToOne,
                            ),
                        ]),
                    )
                }

                #(#forwards)*
                #(#as_include)*
            }
        }
    }))
}

/// Unchanged in behaviour from the previous emitter — only its home moved.
/// Eligible shape: a to-one relation whose `@relation(references:[<col>])`
/// names the related model's primary key.
fn generate_as_include_method(
    model: &Model,
    relation_field: &Field,
    related_model: &Model,
) -> Result<Option<proc_macro2::TokenStream>, String> {
    let Some(parsed) = parse_relation_attribute(relation_field) else {
        return Ok(None);
    };
    if parsed.fields.len() != 1 || parsed.references.len() != 1 {
        return Ok(None);
    }
    let Some(related_pk) = related_model
        .fields
        .iter()
        .find(|field| crate::shared::is_primary_key(field))
    else {
        return Ok(None);
    };
    if parsed.references[0] != related_pk.name {
        return Ok(None);
    }
    let Some(fk_field) = model
        .fields
        .iter()
        .find(|field| field.name == parsed.fields[0])
    else {
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
    let fk_extract_body = if fk_field.ty.arity == TypeArity::Optional {
        quote! { m.#fk_field_ident.clone() }
    } else {
        quote! { ::std::option::Option::Some(m.#fk_field_ident.clone()) }
    };

    Ok(Some(quote! {
        /// Build a `RelationInclude` for this to-one relation.
        pub fn as_include(self) -> ::cratestack::RelationInclude<
            super::super::models::#parent_ident,
            super::super::models::#related_ident,
            #related_pk_type,
        > {
            ::cratestack::RelationInclude {
                parent_fk_extract: |m: &super::super::models::#parent_ident| #fk_extract_body,
                related_descriptor: &super::super::#related_descriptor_ident,
            }
        }
    }))
}
