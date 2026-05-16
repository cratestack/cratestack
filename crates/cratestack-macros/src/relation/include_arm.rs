//! `include=<rel>` serializer-arm emitter — for each relation,
//! produces a match arm that fetches the related rows (via the typed
//! delegate) and writes a child JSON value into the parent's
//! serialized object. Three shapes: to-many (Array), nullable to-one
//! (Null on missing FK), and required to-one.

use cratestack_core::{Field, Model, TypeArity};
use quote::quote;

use crate::shared::{find_model, ident, to_snake_case};

use super::parse::parse_relation_attribute;

pub(crate) fn generate_relation_include_arm(
    model: &Model,
    relation_field: &Field,
    models: &[Model],
    _project_serialized_value_ident: &syn::Ident,
) -> Result<proc_macro2::TokenStream, String> {
    let relation = parse_relation_attribute(relation_field).ok_or_else(|| {
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

    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` on `{}` references unknown model `{}`",
            relation_field.name, model.name, relation_field.ty.name,
        )
    })?;
    let include_name = &relation_field.name;
    let model_name = &model.name;
    let target_accessor_ident = ident(&to_snake_case(&target_model.name));
    let target_field_module_ident = ident(&to_snake_case(&target_model.name));
    let target_field_fn_ident = ident(&relation.references[0]);
    let local_field_ident = ident(&relation.fields[0]);
    let target_serialize_ident = ident(&format!(
        "serialize_{}_model_value",
        to_snake_case(&target_model.name)
    ));

    if relation_field.ty.arity == TypeArity::List {
        return Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_records = db
                    .#target_accessor_ident()
                    .find_many()
                    .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(record.#local_field_ident.clone()))
                    .run(ctx)
                    .await?;
                let mut related_value = Vec::with_capacity(related_records.len());
                for related_record in &related_records {
                    related_value.push(#target_serialize_ident(db, ctx, related_record, &child_selection).await?);
                }
                let related_value = ::cratestack::serde_json::Value::Array(related_value);
                object.insert(#include_name.to_owned(), related_value);
            }
        });
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

    if local_field.ty.arity == TypeArity::Optional {
        Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_value = match record.#local_field_ident.clone() {
                    Some(value) => {
                        let related_record = db
                            .#target_accessor_ident()
                            .find_many()
                            .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(value))
                            .run(ctx)
                            .await?
                            .into_iter()
                            .next();
                        match related_record {
                            Some(related_record) => #target_serialize_ident(db, ctx, &related_record, &child_selection).await?,
                            None => ::cratestack::serde_json::Value::Null,
                        }
                    }
                    None => ::cratestack::serde_json::Value::Null,
                };
                object.insert(#include_name.to_owned(), related_value);
            }
        })
    } else {
        Ok(quote! {
            #include_name => {
                let child_selection = selection.selection_for_include(#include_name).ok_or_else(|| {
                    CoolError::Internal(format!(
                        "validated selection for '{}.{}' is missing child selection",
                        #model_name,
                        #include_name,
                    ))
                })?;
                let related_record = db
                    .#target_accessor_ident()
                    .find_many()
                    .where_(super::#target_field_module_ident::#target_field_fn_ident().eq(record.#local_field_ident.clone()))
                    .run(ctx)
                    .await?
                    .into_iter()
                    .next();
                let related_value = match related_record {
                    Some(related_record) => #target_serialize_ident(db, ctx, &related_record, &child_selection).await?,
                    None => ::cratestack::serde_json::Value::Null,
                };
                object.insert(#include_name.to_owned(), related_value);
            }
        })
    }
}
