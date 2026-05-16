//! Match-arm emitters for the generated `validate_<model>_include_path`
//! and `validate_<model>_include_fields_path` recursive validators —
//! one arm per relation field per parent.

use cratestack_core::{Field, Model};
use quote::quote;

use crate::shared::{find_model, ident, model_name_set, scalar_model_fields, to_snake_case};

pub(crate) fn generate_relation_include_path_validation_arm(
    relation_field: &Field,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let include_name = &relation_field.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` references unknown model `{}`",
            relation_field.name, relation_field.ty.name,
        )
    })?;
    let target_validate_include_path_ident = ident(&format!(
        "validate_{}_include_path",
        to_snake_case(&target_model.name)
    ));
    let target_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&target_model.name).to_uppercase()
    ));

    Ok(quote! {
        (#include_name, Some(rest)) => {
            #target_validate_include_path_ident(rest, &super::models::#target_descriptor_ident)
        }
    })
}

pub(crate) fn generate_relation_include_fields_validation_arm(
    relation_field: &Field,
    model: &Model,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let include_name = &relation_field.name;
    let target_model = find_model(models, &relation_field.ty.name).ok_or_else(|| {
        format!(
            "relation field `{}` references unknown model `{}`",
            relation_field.name, relation_field.ty.name,
        )
    })?;
    let model_names = model_name_set(models);
    let allowed_fields = scalar_model_fields(target_model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let target_validate_include_fields_path_ident = ident(&format!(
        "validate_{}_include_fields_path",
        to_snake_case(&target_model.name)
    ));
    let target_descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&target_model.name).to_uppercase()
    ));
    let parent_model_name = &model.name;

    Ok(quote! {
        (#include_name, Some(rest)) => {
            #target_validate_include_fields_path_ident(rest, fields, &super::models::#target_descriptor_ident)
        }
        (#include_name, None) => {
            for field in fields {
                match field.as_str() {
                    #(#allowed_fields)|* => {}
                    _ => return Err(CoolError::Validation(format!(
                        "unsupported includeFields[{}] selection '{}' for {}.{}",
                        include,
                        field,
                        #parent_model_name,
                        #include_name,
                    ))),
                }
            }
            Ok(())
        }
    })
}
