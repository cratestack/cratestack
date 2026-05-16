//! `create_defaults` builder — for each field with an
//! auth-derived default attribute, emit a `CreateDefault` const-eval
//! struct binding the column → auth field → kind.

use cratestack_core::{Model, TypeArity, TypeDecl};
use quote::quote;

use crate::shared::{auth_default_field, model_name_set, scalar_model_fields, to_snake_case};

pub(super) fn collect_create_defaults(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<Vec<proc_macro2::TokenStream>, String> {
    scalar_model_fields(model, &model_name_set(models))
        .into_iter()
        .filter_map(|field| {
            let auth_field = auth_default_field(field)?;
            let column = to_snake_case(&field.name);
            let auth_field_decl =
                crate::policy::find_auth_field(auth, types, auth_field).map_err(|_| {
                    format!(
                        "auth-derived default on `{}.{}` references unknown auth field `{}`",
                        model.name, field.name, auth_field
                    )
                });
            let kind = match field.ty.name.as_str() {
                "String" | "Cuid" => Ok(quote! { ::cratestack::CreateDefaultType::String }),
                "Int" => Ok(quote! { ::cratestack::CreateDefaultType::Int }),
                "Boolean" => Ok(quote! { ::cratestack::CreateDefaultType::Bool }),
                other => Err(format!(
                    "auth-derived defaults currently support only String/Cuid, Int, and Boolean fields; `{}`.{} is unsupported",
                    model.name, other
                )),
            };
            let nullable = matches!(field.ty.arity, TypeArity::Optional);
            Some(auth_field_decl.and_then(|auth_field_decl| {
                if auth_field_decl.ty.name != field.ty.name
                    && !(field.ty.name == "Cuid" && auth_field_decl.ty.name == "String")
                {
                    return Err(format!(
                        "auth-derived default on `{}.{}` requires matching auth/model field types",
                        model.name, field.name
                    ));
                }

                kind.map(|kind| {
                    quote! {
                        ::cratestack::CreateDefault {
                            column: #column,
                            auth_field: #auth_field,
                            ty: #kind,
                            nullable: #nullable,
                        }
                    }
                })
            }))
        })
        .collect()
}
