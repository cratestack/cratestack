use std::collections::BTreeSet;

use cratestack_core::{Field, Model, TypeArity, TypeDecl};
use quote::quote;

use crate::event::model_emitted_events;
use crate::policy::{
    generate_denies_for_action, generate_denies_for_actions, generate_policies_for_action,
    generate_policies_for_actions,
};
use crate::relation::{collect_allowed_sort_keys, generate_relation_order_module};
use crate::shared::{
    auth_default_field, create_sql_value, doc_attrs, generated_doc_attr, ident,
    is_generated_on_create, is_primary_key, model_name_set, pluralize, relation_model_fields,
    rust_type_tokens, rust_type_tokens_with_scope, scalar_model_fields, to_snake_case,
    update_sql_value,
};

pub(crate) fn generate_model_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let docs = doc_attrs(&model.docs);
    let scalar_fields = scalar_model_fields(model, model_names);
    let fields = scalar_fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let row_fields = scalar_fields
        .iter()
        .map(|field| row_field_tokens(field, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }

        impl<'r> sqlx::FromRow<'r, sqlx::postgres::PgRow> for #model_ident {
            fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
                use sqlx::Row;

                Ok(Self {
                    #(#row_fields)*
                })
            }
        }
    }
}

pub(crate) fn generate_client_model_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let model_ident = ident(&model.name);
    let docs = doc_attrs(&model.docs);
    let scalar_fields = scalar_model_fields(model, model_names);
    let fields = scalar_fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #model_ident {
            #(#fields)*
        }
    }
}

pub(crate) fn generate_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_generated_on_create(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| create_sql_value(field, enum_names));
    let model_ident = ident(&model.name);

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::CreateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                vec![#(#sql_values),*]
            }
        }
    }
}

pub(crate) fn generate_client_create_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Create{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated create input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_generated_on_create(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, false, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}

pub(crate) fn generate_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_primary_key(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));
    let sql_values = fields
        .iter()
        .map(|field| update_sql_value(field, enum_names));
    let model_ident = ident(&model.name);

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }

        impl ::cratestack::UpdateModelInput<super::models::#model_ident> for #input_ident {
            fn sql_values(&self) -> Vec<::cratestack::SqlColumnValue> {
                let mut values = Vec::new();
                #(#sql_values)*
                values
            }
        }
    }
}

pub(crate) fn generate_client_update_input_struct(
    model: &Model,
    model_names: &BTreeSet<&str>,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let input_ident = ident(&format!("Update{}Input", model.name));
    let docs = generated_doc_attr(format!("Generated update input for `{}`.", model.name));
    let fields: Vec<_> = scalar_model_fields(model, model_names)
        .into_iter()
        .filter(|field| !is_primary_key(field))
        .collect();
    let definitions = fields
        .iter()
        .map(|field| struct_field_definition(field, true, enum_names));

    quote! {
        #docs
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
        pub struct #input_ident {
            #(#definitions)*
        }
    }
}

fn row_field_tokens(field: &Field, enum_names: &BTreeSet<&str>) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let field_name = &field.name;
    if !enum_names.contains(field.ty.name.as_str()) {
        return quote! {
            #field_ident: row.try_get(#field_name)?,
        };
    }

    let enum_ident = ident(&field.ty.name);
    let parse_error = |error: proc_macro2::TokenStream| {
        quote! {
            sqlx::Error::Decode(Box::new(::std::io::Error::new(
                ::std::io::ErrorKind::InvalidData,
                #error,
            )))
        }
    };

    match field.ty.arity {
        TypeArity::Required => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: String = row.try_get(#field_name)?;
                    raw.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)?
                },
            }
        }
        TypeArity::Optional => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Option<String> = row.try_get(#field_name)?;
                    raw.map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error)).transpose()?
                },
            }
        }
        TypeArity::List => {
            let decode_error = parse_error(quote! { error });
            quote! {
                #field_ident: {
                    let raw: Vec<String> = row.try_get(#field_name)?;
                    raw.into_iter()
                        .map(|value| value.parse::<super::types::#enum_ident>().map_err(|error| #decode_error))
                        .collect::<Result<Vec<_>, sqlx::Error>>()?
                },
            }
        }
    }
}

fn struct_field_definition(
    field: &Field,
    wrap_for_patch: bool,
    enum_names: &BTreeSet<&str>,
) -> proc_macro2::TokenStream {
    let field_ident = ident(&field.name);
    let docs = doc_attrs(&field.docs);
    let base_type = if enum_names.contains(field.ty.name.as_str()) {
        let enum_ident = ident(&field.ty.name);
        match field.ty.arity {
            TypeArity::Required => quote! { super::types::#enum_ident },
            TypeArity::Optional => quote! { Option<super::types::#enum_ident> },
            TypeArity::List => quote! { Vec<super::types::#enum_ident> },
        }
    } else {
        rust_type_tokens_with_scope(&field.ty, true)
    };
    let field_type = if wrap_for_patch {
        quote! { Option<#base_type> }
    } else {
        base_type
    };

    quote! {
        #docs
        pub #field_ident: #field_type,
    }
}

pub(crate) fn generate_model_descriptor(
    model: &Model,
    models: &[Model],
    types: &[TypeDecl],
    auth: Option<&cratestack_core::AuthBlock>,
) -> Result<proc_macro2::TokenStream, String> {
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let table_name = pluralize(&to_snake_case(&model.name));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);
    let primary_key_sql = to_snake_case(&primary_key.name);
    let read_policies =
        generate_policies_for_actions(model, models, types, auth, &["list", "read"])?;
    let detail_policies =
        generate_policies_for_actions(model, models, types, auth, &["detail", "read"])?;
    let create_policies = generate_policies_for_action(model, models, types, auth, "create")?;
    let create_deny_policies = generate_denies_for_action(model, models, types, auth, "create")?;
    let update_policies = generate_policies_for_action(model, models, types, auth, "update")?;
    let update_deny_policies = generate_denies_for_action(model, models, types, auth, "update")?;
    let delete_policies = generate_policies_for_action(model, models, types, auth, "delete")?;
    let delete_deny_policies = generate_denies_for_action(model, models, types, auth, "delete")?;
    let read_deny_policies =
        generate_denies_for_actions(model, models, types, auth, &["list", "read"])?;
    let detail_deny_policies =
        generate_denies_for_actions(model, models, types, auth, &["detail", "read"])?;
    let create_defaults = scalar_model_fields(model, &model_name_set(models))
        .into_iter()
        .filter_map(|field| {
            let auth_field = auth_default_field(field)?;
            let column = to_snake_case(&field.name);
            let auth_field_decl = crate::policy::find_auth_field(auth, types, auth_field).map_err(|_| {
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
                if auth_field_decl.ty.name != field.ty.name && !(field.ty.name == "Cuid" && auth_field_decl.ty.name == "String") {
                    return Err(format!(
                        "auth-derived default on `{}.{}` requires matching auth/model field types",
                        model.name, field.name
                    ));
                }

                kind.map(|kind| quote! {
                ::cratestack::CreateDefault {
                    column: #column,
                    auth_field: #auth_field,
                    ty: #kind,
                    nullable: #nullable,
                }
                })
            }))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let emitted_events = model_emitted_events(model)?
        .into_iter()
        .map(|operation| match operation {
            cratestack_core::ModelEventKind::Created => {
                quote! { ::cratestack::ModelEventKind::Created }
            }
            cratestack_core::ModelEventKind::Updated => {
                quote! { ::cratestack::ModelEventKind::Updated }
            }
            cratestack_core::ModelEventKind::Deleted => {
                quote! { ::cratestack::ModelEventKind::Deleted }
            }
        })
        .collect::<Vec<_>>();
    let model_names = model_name_set(models);
    let columns = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let rust_name = &field.name;
            let sql_name = to_snake_case(&field.name);
            quote! {
                ::cratestack::ModelColumn {
                    rust_name: #rust_name,
                    sql_name: #sql_name,
                }
            }
        });
    let allowed_fields = scalar_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let allowed_includes = relation_model_fields(model, &model_names)
        .into_iter()
        .map(|field| {
            let name = &field.name;
            quote! { #name }
        })
        .collect::<Vec<_>>();
    let allowed_sorts = collect_allowed_sort_keys(model, models)?
        .into_iter()
        .map(|field| quote! { #field })
        .collect::<Vec<_>>();

    Ok(quote! {
        pub const #descriptor_ident: ::cratestack::ModelDescriptor<#model_ident, #primary_key_type> =
            ::cratestack::ModelDescriptor::new(
                stringify!(#model_ident),
                #table_name,
                &[#(#columns),*],
                #primary_key_sql,
                &[#(#allowed_fields),*],
                &[#(#allowed_includes),*],
                &[#(#allowed_sorts),*],
                &[#(#read_policies),*],
                &[#(#read_deny_policies),*],
                &[#(#detail_policies),*],
                &[#(#detail_deny_policies),*],
                &[#(#create_policies),*],
                &[#(#create_deny_policies),*],
                &[#(#update_policies),*],
                &[#(#update_deny_policies),*],
                &[#(#delete_policies),*],
                &[#(#delete_deny_policies),*],
                &[#(#create_defaults),*],
                &[#(#emitted_events),*],
            );
    })
}

pub(crate) fn generate_field_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let module_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let field_fns = scalar_model_fields(model, model_names).into_iter().map(|field| {
        let function_ident = ident(&field.name);
        let field_type = rust_type_tokens(&field.ty);
        let column = to_snake_case(&field.name);

        quote! {
            #[allow(non_snake_case)]
            pub fn #function_ident() -> ::cratestack::FieldRef<super::models::#model_ident, #field_type> {
                ::cratestack::FieldRef::new(#column)
            }
        }
    });
    let relation_root_fns = relation_model_fields(model, model_names)
        .into_iter()
        .map(|field| {
            let function_ident = ident(&field.name);
            let module_ident = ident(&field.name);

            quote! {
                #[allow(non_snake_case)]
                pub fn #function_ident() -> self::#module_ident::Path {
                    self::#module_ident::Path
                }
            }
        });
    let relation_modules = relation_model_fields(model, model_names)
        .into_iter()
        .map(|relation_field| generate_relation_order_module(model, relation_field, models))
        .collect::<Result<Vec<_>, String>>()?;
    let selection_module = generate_selection_module(model, model_names, models)?;

    Ok(quote! {
        pub mod #module_ident {
            pub use super::*;

            #(#field_fns)*
            #(#relation_root_fns)*
            pub fn select() -> self::selection::Selection {
                self::selection::Selection::default()
            }

            pub fn include_selection() -> self::selection::IncludeSelection {
                self::selection::IncludeSelection::default()
            }

            #(#relation_modules)*
            #selection_module
        }
    })
}

pub(crate) fn generate_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let descriptor_ident = ident(&format!(
        "{}_MODEL",
        to_snake_case(&model.name).to_uppercase()
    ));
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ModelDelegate<'_, models::#model_ident, #primary_key_type> {
            ::cratestack::ModelDelegate::new(&self.runtime, &models::#descriptor_ident)
        }
    }
}

pub(crate) fn generate_bound_model_accessor(model: &Model) -> proc_macro2::TokenStream {
    let method_ident = ident(&to_snake_case(&model.name));
    let model_ident = ident(&model.name);
    let primary_key = model
        .fields
        .iter()
        .find(|field| is_primary_key(field))
        .expect("validated model must have primary key");
    let primary_key_type = rust_type_tokens(&primary_key.ty);

    quote! {
        pub fn #method_ident(&self) -> ::cratestack::ScopedModelDelegate<'_, models::#model_ident, #primary_key_type> {
            self.inner.#method_ident().bind(self.ctx.clone())
        }
    }
}

fn generate_selection_module(
    model: &Model,
    model_names: &BTreeSet<&str>,
    _models: &[Model],
) -> Result<proc_macro2::TokenStream, String> {
    let model_name = &model.name;
    let scalar_fields = scalar_model_fields(model, model_names);
    let selection_field_methods = scalar_fields
        .iter()
        .map(|field| {
            let method_ident = ident(&field.name);
            let field_name = &field.name;
            quote! {
                #[allow(non_snake_case)]
                pub fn #method_ident(mut self) -> Self {
                    self.fields
                        .get_or_insert_with(::std::collections::BTreeSet::new)
                        .insert(#field_name);
                    self
                }
            }
        })
        .collect::<Vec<_>>();
    let include_selection_field_methods = selection_field_methods.clone();
    let selected_scalar_accessors = scalar_fields
        .iter()
        .map(|field| {
            let method_ident = ident(&field.name);
            let field_name = &field.name;
            let field_type = rust_type_tokens(&field.ty);
            quote! {
                #[allow(non_snake_case)]
                pub fn #method_ident(&self) -> Result<#field_type, ::cratestack::CoolError> {
                    decode_projected_field::<#field_type>(
                        &self.fields,
                        self.allows_field(#field_name),
                        #model_name,
                        #field_name,
                    )
                }
            }
        })
        .collect::<Vec<_>>();
    let included_scalar_accessors = selected_scalar_accessors.clone();

    let relation_entries = relation_model_fields(model, model_names)
        .into_iter()
        .map(|field| {
            let include_method_ident = ident(&format!("include_{}", field.name));
            let include_selected_method_ident = ident(&format!("include_{}_selected", field.name));
            let include_name = field.name.clone();
            let include_field_ident = ident(&field.name);
            let target_module_ident = ident(&to_snake_case(&field.ty.name));
            let target_include_selection = quote! { super::super::#target_module_ident::selection::IncludeSelection };
            let relation_accessor = if field.ty.arity == TypeArity::List {
                quote! {
                    #[allow(non_snake_case)]
                    pub fn #include_field_ident(
                        &self,
                    ) -> Result<Vec<super::super::#target_module_ident::selection::ProjectedInclude>, ::cratestack::CoolError> {
                        let selection = self.selection.includes.#include_field_ident.as_ref().ok_or_else(|| {
                            ::cratestack::CoolError::Validation(format!(
                                "include '{}' was not selected for {}",
                                #include_name,
                                #model_name,
                            ))
                        })?;
                        let value = self.fields.get(#include_name).cloned().ok_or_else(|| {
                            ::cratestack::CoolError::Internal(format!(
                                "projected {} payload is missing include '{}'",
                                #model_name,
                                #include_name,
                            ))
                        })?;
                        match value {
                            ::cratestack::serde_json::Value::Array(values) => values
                                .into_iter()
                                .map(|value| {
                                    super::super::#target_module_ident::selection::ProjectedInclude::from_value(
                                        value,
                                        selection.as_ref().clone(),
                                    )
                                })
                                .collect(),
                            other => Err(::cratestack::CoolError::Internal(format!(
                                "projected include '{}.{}' must be an array, got {other:?}",
                                #model_name,
                                #include_name,
                            ))),
                        }
                    }
                }
            } else {
                quote! {
                    #[allow(non_snake_case)]
                    pub fn #include_field_ident(
                        &self,
                    ) -> Result<Option<super::super::#target_module_ident::selection::ProjectedInclude>, ::cratestack::CoolError> {
                        let selection = self.selection.includes.#include_field_ident.as_ref().ok_or_else(|| {
                            ::cratestack::CoolError::Validation(format!(
                                "include '{}' was not selected for {}",
                                #include_name,
                                #model_name,
                            ))
                        })?;
                        let value = self.fields.get(#include_name).cloned().ok_or_else(|| {
                            ::cratestack::CoolError::Internal(format!(
                                "projected {} payload is missing include '{}'",
                                #model_name,
                                #include_name,
                            ))
                        })?;
                        match value {
                            ::cratestack::serde_json::Value::Null => Ok(None),
                            other => super::super::#target_module_ident::selection::ProjectedInclude::from_value(
                                other,
                                selection.as_ref().clone(),
                            )
                            .map(Some),
                        }
                    }
                }
            };

            Ok::<_, String>((
                quote! {
                    #[allow(non_snake_case)]
                    pub fn #include_method_ident(mut self) -> Self {
                        self.includes.#include_field_ident = Some(Box::new(#target_include_selection::default()));
                        self
                    }

                    #[allow(non_snake_case)]
                    pub fn #include_selected_method_ident(
                        mut self,
                        selection: #target_include_selection,
                    ) -> Self {
                        self.includes.#include_field_ident = Some(Box::new(selection));
                        self
                    }
                },
                quote! { pub #include_field_ident: Option<Box<#target_include_selection>>, },
                quote! {
                    if let Some(selection) = &self.includes.#include_field_ident {
                        let prefix = #include_name;
                        query.includes.push(prefix.to_owned());
                        let include_query = selection.to_query();
                        if !include_query.fields.is_empty() {
                            query.include_fields.insert(prefix.to_owned(), include_query.fields);
                        }
                        for nested_include in include_query.includes {
                            query.includes.push(format!("{prefix}.{nested_include}"));
                        }
                        for (path, fields) in include_query.include_fields {
                            query.include_fields.insert(format!("{prefix}.{path}"), fields);
                        }
                    }
                },
                relation_accessor,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;

    let include_methods = relation_entries
        .iter()
        .map(|(methods, _, _, _)| methods.clone())
        .collect::<Vec<_>>();
    let include_fields = relation_entries
        .iter()
        .map(|(_, field, _, _)| field.clone())
        .collect::<Vec<_>>();
    let include_query_steps = relation_entries
        .iter()
        .map(|(_, _, step, _)| step.clone())
        .collect::<Vec<_>>();
    let include_accessors = relation_entries
        .into_iter()
        .map(|(_, _, _, accessor)| accessor)
        .collect::<Vec<_>>();

    Ok(quote! {
        pub mod selection {
            #[derive(Debug, Clone, Default)]
            pub struct Includes {
                #(#include_fields)*
            }

            #[derive(Debug, Clone, Default)]
            pub struct Selection {
                fields: Option<::std::collections::BTreeSet<&'static str>>,
                includes: Includes,
            }

            impl Selection {
                pub fn all_fields(mut self) -> Self {
                    self.fields = None;
                    self
                }

                #(#selection_field_methods)*
                #(#include_methods)*

                pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                    let mut query = ::cratestack::SelectionQuery::default();
                    if let Some(fields) = &self.fields {
                        query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                    }
                    #(#include_query_steps)*
                    query
                }

                pub fn decode_one(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Projected, ::cratestack::CoolError> {
                    Projected::from_value(value, self.clone())
                }

                pub fn decode_many(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Vec<Projected>, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Array(values) => values
                            .into_iter()
                            .map(|value| self.decode_one(value))
                            .collect(),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected {} list payload must be an array, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                pub fn decode_page(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<::cratestack::Page<Projected>, ::cratestack::CoolError> {
                    let page = ::cratestack::serde_json::from_value::<::cratestack::Page<::cratestack::serde_json::Value>>(value)
                        .map_err(|error| ::cratestack::CoolError::Codec(format!(
                            "failed to decode projected {} page payload: {error}",
                            #model_name,
                        )))?;
                    let items = page
                        .items
                        .into_iter()
                        .map(|value| self.decode_one(value))
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(::cratestack::Page::new(items, page.page_info).with_total_count(page.total_count))
                }
            }

            impl ::cratestack::client_rust::Projection for Selection {
                type Output = Projected;

                fn selection_query(&self) -> ::cratestack::SelectionQuery {
                    self.to_query()
                }

                fn decode_one(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Self::Output, ::cratestack::CoolError> {
                    Selection::decode_one(self, value)
                }

                fn decode_many(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<Vec<Self::Output>, ::cratestack::CoolError> {
                    Selection::decode_many(self, value)
                }

                fn decode_page(
                    &self,
                    value: ::cratestack::serde_json::Value,
                ) -> Result<::cratestack::Page<Self::Output>, ::cratestack::CoolError> {
                    Selection::decode_page(self, value)
                }
            }

            #[derive(Debug, Clone, Default)]
            pub struct IncludeSelection {
                fields: Option<::std::collections::BTreeSet<&'static str>>,
                includes: Includes,
            }

            impl IncludeSelection {
                pub fn all_fields(mut self) -> Self {
                    self.fields = None;
                    self
                }

                #(#include_selection_field_methods)*
                #(#include_methods)*

                pub fn to_query(&self) -> ::cratestack::SelectionQuery {
                    let mut query = ::cratestack::SelectionQuery::default();
                    if let Some(fields) = &self.fields {
                        query.fields = fields.iter().map(|field| (*field).to_owned()).collect();
                    }
                    #(#include_query_steps)*
                    query
                }
            }

            #[derive(Debug, Clone)]
            pub struct Projected {
                fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selection: Selection,
            }

            impl Projected {
                fn from_value(
                    value: ::cratestack::serde_json::Value,
                    selection: Selection,
                ) -> Result<Self, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected {} payload must be an object, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                fn allows_field(&self, field: &str) -> bool {
                    match &self.selection.fields {
                        Some(fields) => fields.contains(field),
                        None => true,
                    }
                }

                pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                    &self.fields
                }

                #(#selected_scalar_accessors)*
                #(#include_accessors)*
            }

            #[derive(Debug, Clone)]
            pub struct ProjectedInclude {
                fields: ::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selection: IncludeSelection,
            }

            impl ProjectedInclude {
                pub(crate) fn from_value(
                    value: ::cratestack::serde_json::Value,
                    selection: IncludeSelection,
                ) -> Result<Self, ::cratestack::CoolError> {
                    match value {
                        ::cratestack::serde_json::Value::Object(fields) => Ok(Self { fields, selection }),
                        other => Err(::cratestack::CoolError::Internal(format!(
                            "projected included {} payload must be an object, got {other:?}",
                            #model_name,
                        ))),
                    }
                }

                fn allows_field(&self, field: &str) -> bool {
                    match &self.selection.fields {
                        Some(fields) => fields.contains(field),
                        None => true,
                    }
                }

                pub fn raw(&self) -> &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value> {
                    &self.fields
                }

                #(#included_scalar_accessors)*
                #(#include_accessors)*
            }

            fn decode_projected_field<T>(
                object: &::cratestack::serde_json::Map<String, ::cratestack::serde_json::Value>,
                selected: bool,
                model_name: &str,
                field_name: &str,
            ) -> Result<T, ::cratestack::CoolError>
            where
                T: ::cratestack::serde::de::DeserializeOwned,
            {
                if !selected {
                    return Err(::cratestack::CoolError::Validation(format!(
                        "field '{}.{}' was not selected",
                        model_name,
                        field_name,
                    )));
                }

                let value = object.get(field_name).cloned().ok_or_else(|| {
                    ::cratestack::CoolError::Internal(format!(
                        "projected {} payload is missing field '{}'",
                        model_name,
                        field_name,
                    ))
                })?;

                ::cratestack::serde_json::from_value(value).map_err(|error| {
                    ::cratestack::CoolError::Internal(format!(
                        "failed to decode projected field '{}.{}': {error}",
                        model_name,
                        field_name,
                    ))
                })
            }
        }
    })
}
